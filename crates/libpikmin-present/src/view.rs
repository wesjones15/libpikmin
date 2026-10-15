//! A sim posed for a host that draws with its own engine: the disc's models (loaded once, for
//! the host to build meshes and textures from) and, every frame, a draw list of posed and
//! CPU-skinned instances. The host only uploads vertices; which animation plays, where, and in
//! what colour is decided here, the way the viewer decides it.
//!
//! Instances are in sim units and axes. Each one has a position and rotation; its vertices are
//! in that local frame (scale already applied), so a host places one object per instance.

use std::collections::{HashMap, HashSet};

use glam::{Mat3, Mat4, Quat, Vec3};
use libpikmin::{Event, PelletState, PikiState, Sim, SproutState};
use libpikmin_assets::pose::srt_matrix;
use libpikmin_assets::{AnimSet, Assets, Batch, Blend, Model, Skeleton, paths};

use crate::animator::{Animator, piki_look, piki_rgb};
use crate::effects::{Effects, Flat, WhistleFrame};
use crate::tev::{self, Inputs};
use libpikmin_assets::CollSphere;
use libpikmin_assets::particle::effects as fx_defs;

/// How fast a locked-on cursor turns, radians per second.
const LOCK_SPIN: f32 = 2.5;

/// Models, in the order hosts see them.
pub mod models {
    pub const OLIMAR: u32 = 0;
    /// Blue, Red, Yellow (`PikiColor` order).
    pub const PIKMIN: [u32; 3] = [1, 2, 3];
    /// Leaf, bud, flower.
    pub const HAPPA: [u32; 3] = [4, 5, 6];
    pub const ONION: u32 = 7;
    /// Numbered pellets 1, 5, 10, 20.
    pub const PELLETS: [u32; 4] = [8, 9, 10, 11];
    pub const PIKIHEAD: u32 = 12;
    pub const CURSOR: u32 = 13;
    pub const MARKER: u32 = 14;
    pub const SHIP: u32 = 15;
    /// The Onions' light cones: Blue, Red, Yellow.
    pub const BEAMS: [u32; 3] = [16, 17, 18];
    pub const COUNT: usize = 19;
}

static REPAIRED_SHIP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Host extension: draw the S.S. Dolphin with the parts the sim has put on it (all 30 is the repaired
/// ship, with its lights and effects) as models 19 and up ([`View::part_model`]). Off by default: the
/// parts' models aren't loaded, which costs time and memory. Call before [`View::load`].
pub fn set_repaired_ship(on: bool) {
    REPAIRED_SHIP.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// What a draw is, for hosts that treat some kinds differently.
pub mod kinds {
    pub const CAPTAIN: u8 = 0;
    pub const PIKI: u8 = 1;
    pub const HAPPA: u8 = 2;
    pub const ONION: u8 = 3;
    pub const PELLET: u8 = 4;
    pub const SPROUT: u8 = 5;
    pub const SPROUT_HAPPA: u8 = 6;
    pub const CURSOR: u8 = 7;
    pub const MARKER: u8 = 8;
    pub const SHIP: u8 = 9;
    /// An Onion's light cone (`GoalItem::mSpotModelEff`).
    pub const BEAM: u8 = 10;
    /// A part on the ship (`models::COUNT` and up, see [`super::View::part_model`]).
    pub const SHIP_PART: u8 = 11;
}

/// Draw flags.
pub mod flags {
    /// Flashing from a hit: draw brighter.
    pub const FLASH: u8 = 1;
    /// Frozen ([`libpikmin::Sim::freeze_pikis_in_sphere`]): hosts may add an icy sheen. Its
    /// colour is already paled and its animation held.
    pub const FROZEN: u8 = 2;
}

/// How a host draws a batch.
pub mod blend {
    pub const OPAQUE: u8 = 0;
    /// Alpha tested (a leaf's outline).
    pub const CUTOUT: u8 = 1;
    /// Blended and unlit (Olimar's helmet): a deferred host's lit shader can't blend.
    pub const TRANSLUCENT: u8 = 2;
    /// Added to the screen by its alpha and unlit, without writing depth (the Onions' light
    /// cones: `GX_BL_SRCALPHA, GX_BL_ONE` from their `PVWPeInfo`).
    pub const ADDITIVE: u8 = 3;
}

/// Blended materials at least this opaque everywhere draw opaque (the pellets are 78%).
const OPAQUE_ENOUGH: f32 = 0.7;

/// Most material variants baked (pellet colours, ...) before new ones reuse the default.
const MAX_TEXTURES: usize = 256;

/// A loaded model.
pub struct ViewModel {
    pub model: Model,
    pub skel: Skeleton,
    pub batches: Vec<Batch>,
    pub vertex_count: usize,
    /// Bind-pose bounds.
    pub lo: Vec3,
    pub hi: Vec3,
    images: tev::Images,
    /// Per material: baked (it reads a texture over the mesh's coordinates).
    textured: Vec<bool>,
    /// Per batch: its baked texture with no per-draw inputs ([`View::textures`]), or -1.
    pub batch_texture: Vec<i32>,
    /// Per batch: [`blend`].
    pub batch_blend: Vec<u8>,
    /// Per batch: environment mapped ([`tev::Baked::env`]): its texture coordinates come from
    /// the view-space normals each frame ([`View::uvs`]), through this matrix.
    pub batch_env: Vec<Option<Option<[[f32; 3]; 2]>>>,
}

impl ViewModel {
    fn load(assets: &Assets, path: &str) -> Result<Self, libpikmin_assets::Error> {
        let model = assets.model(path)?;
        let skel = Skeleton::new(&model);
        let batches = model.batches()?;
        let pose = skel.pose(Mat4::IDENTITY, None);
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for b in &batches {
            for v in &b.vertices {
                let p = pose.skin(v).0;
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let vertex_count = batches.iter().map(|b| b.vertices.len()).sum();
        let images = tev::Images::new(&model);
        let textured = (0..model.materials.len()).map(|m| tev::textured(&model, m, &images)).collect();
        Ok(ViewModel {
            model,
            skel,
            batches,
            vertex_count,
            lo,
            hi,
            images,
            textured,
            batch_texture: Vec::new(),
            batch_blend: Vec::new(),
            batch_env: Vec::new(),
        })
    }

    /// Horizontal radius of the bind pose.
    pub fn radius(&self) -> f32 {
        [self.lo.x, self.lo.z, self.hi.x, self.hi.z]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
            .max(0.01)
    }

    /// How batch `b` is lit: 1 its colour, 2 its alpha (`LightingControlFlags::EnableAlpha0`:
    /// with the stages' ambient alpha of 0, Olimar's helmet is only opaque where light falls).
    pub fn batch_light(&self, b: usize) -> u8 {
        let m = &self.model.materials[self.batches[b].material];
        let alpha = m.flags & libpikmin_assets::model::mat_flags::PVW != 0 && m.lighting & 4 != 0;
        m.lit() as u8 | (alpha as u8) << 1
    }
}

/// Baked materials, shared by every model: each (model, material, inputs that change it)
/// once.
#[derive(Default)]
pub struct Textures {
    pub list: Vec<tev::Baked>,
    ids: HashMap<(u32, usize, [u8; 16]), i32>,
}

impl Textures {
    /// The texture for `material` of `vm` with `inputs`, baking it the first time; -1 if the
    /// material isn't textured.
    fn get(&mut self, model: u32, vm: &ViewModel, material: usize, inputs: Inputs) -> i32 {
        if !vm.textured[material] {
            return -1;
        }
        let k = (model, material, tev::key(&vm.model, material, inputs));
        if let Some(&id) = self.ids.get(&k) {
            return id;
        }
        if self.list.len() >= MAX_TEXTURES {
            return self.get(model, vm, material, Inputs::default());
        }
        let id = match tev::bake(&vm.model, material, inputs, &vm.images) {
            Some(b) => {
                self.list.push(b);
                self.list.len() as i32 - 1
            }
            None => -1,
        };
        self.ids.insert(k, id);
        id
    }
}

/// One posed instance.
#[derive(Debug, Clone, Copy)]
pub struct Draw {
    /// Stable while the object lives: kind in the top byte, then the object's id.
    pub key: u64,
    pub kind: u8,
    pub flags: u8,
    pub model: u32,
    pub pos: Vec3,
    pub rot: Quat,
    /// Its vertices in [`View::positions`] / [`View::normals`] (the model's vertex count).
    pub first_vertex: u32,
    /// Its batches' colours in [`View::colors`] and textures in [`View::texture_ids`] (one
    /// per batch).
    pub first_color: u32,
    /// 1 opaque .. 0 gone.
    pub alpha: f32,
}

/// A frozen Pikmin's body colour: its own, two thirds of the way to pale ice blue.
fn icy(c: [u8; 4]) -> [u8; 4] {
    const ICE: [f32; 3] = [190.0, 228.0, 255.0];
    let mix = |k: usize| (c[k] as f32 + (ICE[k] - c[k] as f32) * 0.67).round() as u8;
    [mix(0), mix(1), mix(2), c[3]]
}

fn key(kind: u8, id: u64) -> u64 {
    (kind as u64) << 56 | (id & ((1 << 56) - 1))
}

#[derive(Clone, Copy)]
struct Interp {
    prev: Vec3,
    cur: Vec3,
    prev_rot: Quat,
    cur_rot: Quat,
    tick: u64,
}

impl Interp {
    fn new(at: Vec3, rot: Quat, tick: u64) -> Self {
        Self { prev: at, cur: at, prev_rot: rot, cur_rot: rot, tick }
    }

    fn sample(&mut self, at: Vec3, rot: Quat, tick: u64, alpha: f32) -> (Vec3, Quat) {
        if self.tick != tick {
            // A respawn or teleport starts a fresh blend for both position and facing.
            let jumped = (at - self.cur).length() > 200.0;
            self.prev = if jumped { at } else { self.cur };
            self.prev_rot = if jumped { rot } else { self.cur_rot };
            self.cur = at;
            self.cur_rot = rot;
            self.tick = tick;
        }
        (self.prev.lerp(self.cur, alpha), self.prev_rot.slerp(self.cur_rot, alpha))
    }
}

#[cfg(test)]
mod interpolation_tests {
    use super::*;

    #[test]
    fn position_and_facing_share_the_render_fraction() {
        let start = Quat::from_rotation_y(179f32.to_radians());
        let end = Quat::from_rotation_y(-179f32.to_radians());
        let mut state = Interp::new(Vec3::ZERO, start, 1);
        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let (at, rot) = state.sample(Vec3::X * 10.0, end, 2, alpha);
            assert!((at.x - 10.0 * alpha).abs() < 1e-5);
            let expected = Quat::from_rotation_y((179.0 + 2.0 * alpha).to_radians());
            assert!(rot.dot(expected).abs() > 0.99999, "facing took the long path");
        }
        let (at, rot) = state.sample(Vec3::X * 500.0, Quat::IDENTITY, 3, 0.0);
        assert_eq!(at, Vec3::X * 500.0);
        assert_eq!(rot, Quat::IDENTITY);
    }
}

/// A ship part's model and animations (`PelletShapeObject`).
struct PartView {
    model: u32,
    file: String,
    anims: Option<AnimSet>,
    upper: Vec<bool>,
}

struct ShipFlight {
    ground_anchor: Option<Vec3>,
    animator: libpikmin_assets::ship::ShipAnimator,
    progress: Option<f32>,
    flames: Vec<(u8, &'static str)>,
}

// Strip only cinematic travel; the clip still supplies every articulated pose. Hosts
// place the model along their collision-safe trajectory instead of adding both motions.
fn rebase_pose(pose: &mut libpikmin_assets::pose::Pose, offset: Vec3) {
    let translation = Mat4::from_translation(offset);
    pose.root = translation * pose.root;
    for matrix in pose.joints.iter_mut().chain(pose.envelopes.iter_mut()) { *matrix = translation * *matrix; }
}

pub struct View {
    pub models: Vec<ViewModel>,
    piki_anims: AnimSet,
    onion_anims: AnimSet,
    ship_anims: AnimSet,
    ship_mgr: libpikmin_assets::AnimMgr,
    ship_contexts: Vec<usize>,
    /// Which of the ship's level lights animates each of its materials (`LightAnimator`).
    ship_lights: Vec<Option<usize>>,
    /// Each of the 30 ship parts' model, animations and the joints its upper animator drives, when loaded.
    parts: Vec<Option<PartView>>,
    ships: HashMap<u32, ShipFlight>,
    pub ship_sounds: Vec<(libpikmin::onion::OnionId, &'static str, bool)>,
    pub demo_sounds: Vec<Option<(u16, u16)>>,
    pikihead_anims: AnimSet,
    pub animator: Animator,
    pub draws: Vec<Draw>,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Per batch of every draw: what multiplies its texture (white for baked materials, whose
    /// colour is in the texture), alpha included.
    pub colors: Vec<[f32; 4]>,
    /// Per batch of every draw: its texture in [`View::textures`], or -1.
    pub texture_ids: Vec<i32>,
    /// Every instance's texture coordinates this frame, alongside [`View::positions`]: the
    /// model's own, except environment-mapped batches' ([`ViewModel::batch_env`]).
    pub uvs: Vec<[f32; 2]>,
    /// The camera's right, up and backward axes (GX view space), for environment maps.
    camera: [Vec3; 3],
    pub textures: Textures,
    /// The whistle and dead Pikmin's ghosts (none if the effect files didn't load).
    pub effects: Option<Effects>,
    /// [`View::effects`] as quads, after [`View::particles`].
    pub particles: Flat,
    /// Olimar's antenna sphere, where the whistle's beam starts (`Navi::demoDraw`).
    ante: Option<CollSphere>,
    whistles: Vec<WhistleFrame>,
    interp: HashMap<u64, Interp>,
    time: f32,
    /// Draw each captain's cursor and marker (on by default; off for a captain shown on his
    /// own, like a host's character select screen).
    pub show_cursors: bool,
    /// Each Onion's light cone width, 0..1 (`GoalItem::startConeShrink` / `startConeEmit`).
    cones: HashMap<u32, f32>,
    /// Captains appearing in a beam: how far along, 0..1 (see [`View::set_materialize`]).
    materialize: HashMap<u32, f32>,
    /// Captains whose arrival effects are running.
    materializing: HashSet<u32>,
    /// Onions whose beacon rings are on (`GoalItem::setSpotActive`).
    spots: HashSet<u32>,
    /// Onions a pellet just reached, for their `GoalEffect`.
    arrivals: Vec<libpikmin::onion::OnionId>,
    /// Growth and deflowering seen in events, for their effects once the sim's positions are
    /// to hand.
    grown: Vec<Grown>,
    /// The stage each sprout already sparkled for (`PikiHeadAI::WaitExec` once per stage).
    sparkled: HashMap<libpikmin::SproutId, libpikmin::Happa>,
}

enum Grown {
    Sprout(libpikmin::SproutId),
    Piki(libpikmin::PikiId),
    Deflowered(libpikmin::PikiId),
}

/// `PikiHeadAI::WaitExec`: the sparkle starts this many seconds before the next stage.
const SPARKLE_LEAD: f32 = 2.0;
/// How far above its feet a sprout's or Pikmin's growth effect appears (itemAI.cpp).
const GROWTH_FX_LIFT: f32 = 13.0;


/// `GoalItem::updateConeShrink` / `updateConeEmit`: the cone narrows or widens over 0.8 s.
const CONE_TIME: f32 = 0.8;
/// `EffectMgr`'s `geometryLoadInfo` scale for the cone models.
const CONE_SCALE: f32 = 0.1;
/// `GoalEffect::invoke`: no more gravity from this frame of the suck effect.
const SUCK_GRAVITY_FRAMES: f32 = 50.0;

impl View {
    pub fn load(assets: &Assets) -> Result<Self, libpikmin_assets::Error> {
        let mut paths_in_order: Vec<&str> = vec![paths::OLIMAR];
        paths_in_order.extend(paths::PIKMIN);
        paths_in_order.extend(paths::HAPPA);
        paths_in_order.push(paths::ONION);
        paths_in_order.extend(paths::PELLETS);
        paths_in_order.extend([paths::PIKIHEAD, paths::CURSOR, paths::CURSOR_MARKER]);
        paths_in_order.push(libpikmin_assets::ship::SHIP_MODEL);
        paths_in_order.extend(paths::ONION_BEAMS);
        debug_assert_eq!(paths_in_order.len(), models::COUNT);
        let mut models = paths_in_order
            .iter()
            .map(|p| ViewModel::load(assets, p))
            .collect::<Result<Vec<_>, _>>()?;
        let mut parts: Vec<Option<PartView>> = Vec::new();
        if REPAIRED_SHIP.load(std::sync::atomic::Ordering::Relaxed) {
            let infos = assets.read(libpikmin_assets::ship::PEL_ANIM).ok()
                .and_then(|d| libpikmin_assets::ship::parse_pel_anim(&d).ok()).unwrap_or_default();
            for id in libpikmin_assets::ship::PART_IDS {
                let loaded = infos.iter().find(|i| i.id == id).and_then(|info| {
                    let vm = ViewModel::load(assets, &info.model()).ok()?;
                    let anims = AnimSet::load(assets, &info.anim_mgr(), &info.bundle()).ok();
                    let mut upper = vec![false; vm.skel.len()];
                    if info.override_joint >= 0 {
                        for j in vm.skel.subtree(info.override_joint as usize) { upper[j] = true; }
                    }
                    Some((vm, PartView { model: 0, file: info.file.clone(), anims, upper }))
                });
                parts.push(loaded.map(|(vm, mut part)| {
                    part.model = models.len() as u32;
                    models.push(vm);
                    part
                }));
            }
        }
        // Each material baked as it is with no per-draw inputs, which also says how opaque it is.
        let mut textures = Textures::default();
        for (mi, vm) in models.iter_mut().enumerate() {
            let mut ids = Vec::new();
            let mut blends = Vec::new();
            let mut envs = Vec::new();
            for b in &vm.batches {
                let id = textures.get(mi as u32, vm, b.material, Inputs::default());
                let floor = match usize::try_from(id) {
                    Ok(i) => textures.list[i].min_alpha(),
                    Err(_) => tev::flat_color(&vm.model, b.material, Inputs::default(), &vm.images)[3],
                };
                let mat = &vm.model.materials[b.material];
                blends.push(match mat.blend() {
                    _ if mat.additive() => blend::ADDITIVE,
                    Blend::Opaque => blend::OPAQUE,
                    Blend::AlphaTest => blend::CUTOUT,
                    Blend::AlphaBlend | Blend::Inverse if floor >= OPAQUE_ENOUGH => blend::OPAQUE,
                    Blend::AlphaBlend | Blend::Inverse => blend::TRANSLUCENT,
                });
                envs.push(usize::try_from(id).ok().and_then(|i| textures.list[i].env));
                ids.push(id);
            }
            vm.batch_texture = ids;
            vm.batch_blend = blends;
            vm.batch_env = envs;
        }
        // The level lights animate the materials of four joints' subtrees (`UfoItem::UfoItem`).
        let mut ship_lights = vec![None; models[models::SHIP as usize].model.materials.len()];
        for (l, &root) in libpikmin_assets::ship::LIGHT_JOINTS.iter().enumerate() {
            let vm = &models[models::SHIP as usize];
            for j in vm.skel.subtree(root) {
                for &(m, _) in &vm.model.joints[j].matpolys {
                    if let Some(slot) = ship_lights.get_mut(m as usize) { *slot = Some(l); }
                }
            }
        }
        let ante = models[models::OLIMAR as usize].model.coll.iter().find(|c| c.id == "ante").cloned();
        let ship_contexts = models[models::SHIP as usize].skel.override_map(&libpikmin_assets::ship::CONTEXT_JOINTS);
        Ok(View {
            models,
            piki_anims: AnimSet::load(assets, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE)?,
            onion_anims: AnimSet::load(assets, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE)?,
            ship_anims: AnimSet::load(assets, libpikmin_assets::ship::SHIP_ANIM_MGR, libpikmin_assets::ship::SHIP_BUNDLE)?,
            ship_mgr: libpikmin_assets::AnimMgr::parse(&assets.read(libpikmin_assets::ship::SHIP_ANIM_MGR)?)?,
            ship_contexts,
            ship_lights,
            parts,
            ships: HashMap::new(),
            ship_sounds: Vec::new(),
            demo_sounds: Vec::new(),
            pikihead_anims: AnimSet::load(assets, paths::PIKIHEAD_ANIM_MGR, paths::PIKIHEAD_BUNDLE)?,
            animator: Animator::default(),
            draws: Vec::new(),
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            texture_ids: Vec::new(),
            uvs: Vec::new(),
            camera: [Vec3::X, Vec3::Y, Vec3::Z],
            textures,
            effects: Effects::load(assets),
            particles: Flat::default(),
            ante,
            whistles: Vec::new(),
            interp: HashMap::new(),
            time: 0.0,
            show_cursors: true,
            cones: HashMap::new(),
            materialize: HashMap::new(),
            materializing: HashSet::new(),
            spots: HashSet::new(),
            arrivals: Vec::new(),
            grown: Vec::new(),
            sparkled: HashMap::new(),
        })
    }

    pub fn on_event(&mut self, e: &Event) {
        self.animator.on_event(e);
        // `Piki::doKill`: the ghost rises from where it died.
        if let Event::PikiGone { pos, .. } = e
            && let Some(fx) = self.effects.as_mut()
        {
            fx.spawn(fx_defs::PIKI_DEAD_SOUL, *pos);
        }
        if let Event::PelletArrived { onion, .. } = e {
            self.arrivals.push(*onion);
        }
        match e {
            Event::SproutGrew { sprout, .. } => self.grown.push(Grown::Sprout(*sprout)),
            Event::PikiGrew { piki, .. } => self.grown.push(Grown::Piki(*piki)),
            Event::Deflowered { piki } => self.grown.push(Grown::Deflowered(*piki)),
            _ => {}
        }
    }

    /// The sparkles before a sprout grows, the burst when it or a fighting Pikmin does, and
    /// the petals of one that lost a stage (itemAI.cpp `PikiHeadAI`, pikiState.cpp
    /// `PikiFlickState`).
    fn growth_effects(&mut self, sim: &Sim) {
        let grown = std::mem::take(&mut self.grown);
        let Some(fx) = self.effects.as_mut() else {
            self.sparkled.clear();
            return;
        };
        let lift = Vec3::Y * GROWTH_FX_LIFT;
        for sp in sim.sprouts() {
            if let SproutState::Ready { timer } = sp.state
                && timer < SPARKLE_LEAD
                && sp.happa != libpikmin::Happa::Flower
                && sp.claimed_by.is_none()
                && self.sparkled.get(&sp.id) != Some(&sp.happa)
            {
                self.sparkled.insert(sp.id, sp.happa);
                fx.spawn(fx_defs::PIKI_GROW_UP1, sp.pos + lift);
            }
        }
        self.sparkled.retain(|id, _| sim.sprouts().any(|s| s.id == *id));
        for g in grown {
            match g {
                Grown::Sprout(id) => {
                    if let Some(sp) = sim.sprouts().find(|s| s.id == id) {
                        fx.spawn(fx_defs::PIKI_GROW_UP2, sp.pos + lift);
                    }
                }
                Grown::Piki(id) => {
                    fx.spawn(fx_defs::PIKI_GROW_UP2, sim.piki(id).pos + lift);
                }
                Grown::Deflowered(id) => {
                    let at = sim.piki(id).pos + lift;
                    for def in fx_defs::PIKI_DEFLOWER {
                        fx.spawn(def, at);
                    }
                }
            }
        }
    }

    /// An Onion's light cone, beacon rings and arrival effect (`GoalItem::mSpotModelEff`,
    /// `setSpotActive`, `GoalEffect`).
    fn onion_light(&mut self, sim: &Sim, i: usize, o: &libpikmin::Onion, dt: f32) {
        use libpikmin::onion::OnionAnim;
        let colour = o.colors[0].index();
        // Grounded it stands lit; flying, landing or away it closes (`startLand` sets it to 0,
        // the takeoff motion's key starts `startConeShrink`) and opens again once down.
        let grounded = o.grounded();
        let cone = self.cones.entry(o.id.0).or_insert(if grounded { 1.0 } else { 0.0 });
        let step = dt / CONE_TIME;
        *cone = if grounded { (*cone + step).min(1.0) } else { (*cone - step).max(0.0) };
        let width = *cone;
        if width > 0.0 && o.anim != OnionAnim::Away {
            let model = models::BEAMS[colour];
            let skel = &self.models[model as usize].skel;
            let root = Mat4::from_scale(Vec3::new(CONE_SCALE * width, CONE_SCALE, CONE_SCALE * width));
            let pose = skel.pose(root, None);
            let k = key(kinds::BEAM, i as u64);
            self.push(k, kinds::BEAM, model, o.pos, Quat::IDENTITY, pose, &|_| Inputs::default(), 1.0, 0);
        }

        // `NaviWalkState::exec`: a captain at the Onion lights its rings.
        let lit = grounded && sim.captains().iter().any(|c| sim.onion_in_reach(c.id) == Some(o.id));
        let fx_key = (7u64 << 48) | ((o.id.0 as u64) << 8);
        if let Some(e) = self.effects.as_mut() {
            if lit && self.spots.insert(o.id.0) {
                e.attach(fx_key, fx_defs::ONYON_BEACON_RING[colour], o.pos);
                e.attach(fx_key | 1, fx_defs::ONYON_HALO_RING[colour], o.pos);
            } else if !lit && self.spots.remove(&o.id.0) {
                e.detach_where(|k| k & !255 == fx_key);
            }
            if lit {
                e.move_attached(fx_key, o.pos);
                e.move_attached(fx_key | 1, o.pos);
            }
            // `PelletGoalState::init`: `GoalEffect` at the goal for an Onion (the ship's is
            // `UfoSuck`).
            if self.arrivals.contains(&o.id)
                && let Some(g) = e.spawn(fx_defs::ONYON_SUCK, o.goal_pos())
            {
                g.gravity_off_at = Some(SUCK_GRAVITY_FRAMES);
            }
        }
    }

    /// The camera's world axes (`forward` is where it looks), for environment maps and
    /// particles. Call before [`View::update`].
    pub fn set_camera(&mut self, right: Vec3, up: Vec3, forward: Vec3) {
        self.camera = [right, up, -forward];
    }

    /// This frame's particles as quads facing a camera whose world axes are `right` and `up`
    /// (into [`View::particles`]).
    pub fn particles(&mut self, right: Vec3, up: Vec3) {
        match self.effects.as_mut() {
            Some(fx) => self.particles.update(fx, right, up),
            None => self.particles.quads.clear(),
        }
    }

    /// Where an object is between the last two ticks (`alpha` 0..1 into the next tick).
    fn place(&mut self, k: u64, at: Vec3, tick: u64, alpha: f32) -> Vec3 {
        self.place_rot(k, at, Quat::IDENTITY, tick, alpha).0
    }

    fn place_rot(&mut self, k: u64, at: Vec3, rot: Quat, tick: u64, alpha: f32) -> (Vec3, Quat) {
        self.interp.entry(k).or_insert_with(|| Interp::new(at, rot, tick)).sample(at, rot, tick, alpha)
    }

    /// Skins `model` posed by `pose_root`/`anim`, appends a draw, and returns the joints.
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        k: u64,
        kind: u8,
        model: u32,
        pos: Vec3,
        rot: Quat,
        pose: libpikmin_assets::Pose,
        inputs: &dyn Fn(usize) -> Inputs,
        alpha: f32,
        flags: u8,
    ) {
        let vm = &self.models[model as usize];
        let first_vertex = self.positions.len() as u32;
        let first_color = self.colors.len() as u32;
        for (bi, b) in vm.batches.iter().enumerate() {
            let env = vm.batch_env.get(bi).copied().flatten();
            for v in &b.vertices {
                let (p, n) = pose.skin(v);
                let n = n.normalize_or_zero();
                self.positions.push(p);
                self.normals.push(n);
                self.uvs.push(match env {
                    Some(mtx) => {
                        let w = rot * n;
                        let [r, u, back] = self.camera;
                        tev::env_uv(Vec3::new(w.dot(r), w.dot(u), w.dot(back)), mtx)
                    }
                    None => [v.uv.x, v.uv.y],
                });
            }
            let inp = inputs(b.material);
            let id = self.textures.get(model, vm, b.material, inp);
            let mut c = if id >= 0 { [1.0; 4] } else { tev::flat_color(&vm.model, b.material, inp, &vm.images) };
            c[3] *= alpha;
            self.colors.push(c);
            self.texture_ids.push(id);
        }
        self.draws.push(Draw { key: k, kind, flags, model, pos, rot, first_vertex, first_color, alpha });
    }

    /// Captain `captain` is appearing in a beam, `progress` 0 (a thin column) to 1 (himself); 1 or more
    /// ends it. Hosts call it each frame while it lasts.
    pub fn set_materialize(&mut self, captain: u32, progress: f32) {
        if progress.is_finite() && (0.0..1.0).contains(&progress) {
            self.materialize.insert(captain, progress);
        } else {
            self.materialize.remove(&captain);
        }
    }

    /// Poses everything in `sim` for this frame. `alpha` is how far into the next tick the
    /// clock is ([`Sim::alpha`]), `dt` the frame time.
    pub fn update(&mut self, sim: &Sim, alpha: f32, dt: f32) {
        self.draws.clear();
        self.positions.clear();
        self.normals.clear();
        self.colors.clear();
        self.texture_ids.clear();
        self.uvs.clear();
        self.whistles.clear();
        self.time += dt;
        self.animator.begin_frame(dt);
        let tick = sim.tick();
        self.animator.render_clock(tick, alpha);
        let params = sim.params().clone();
        let mut live = HashSet::new();
        let no_inputs = |_: usize| Inputs::default();

        // Onions (`GoalItem`): their colour is a register animation at frame colour * 2
        // (`GoalItem::refresh`). The Dolphin uses UfoAnimator's eight skeleton contexts.
        for (i, o) in sim.onions().iter().enumerate() {
            if o.ship.is_some() {
                use libpikmin_assets::ship::ShipCue;
                if o.anim == libpikmin::onion::OnionAnim::Away { continue; }
                // The parts on the ship, when their models are loaded (`set_repaired_ship`); else it is
                // the bare wreck as before.
                let (on_board, level) = match o.ship.as_ref() {
                    Some(s) if !self.parts.is_empty() => (s.parts.clone(), params.ai.ship_level(s.parts.len() as u32)),
                    _ => (Vec::new(), 0),
                };
                let flight = self.ships.entry(o.id.0).or_insert_with(|| {
                    let part_mgrs = self.parts.iter().map(|p| {
                        let p = p.as_ref()?;
                        Some((p.anims.as_ref()?.mgr.clone(), p.upper.iter().any(|&u| u)))
                    }).collect();
                    let mut animator = libpikmin_assets::ship::ShipAnimator::new(&self.ship_mgr, part_mgrs);
                    animator.start(&on_board, level, &mut Vec::new());
                    ShipFlight { animator, progress: None, flames: Vec::new(), ground_anchor: None }
                });
                let mut cues = Vec::new();
                let landing = o.flight_motion.filter(|m| m.0).map(|m| m.1);
                if let Some(progress) = landing {
                    if flight.progress.is_none_or(|last| progress < last) {
                        cues.push(ShipCue::StopAttached);
                        flight.animator.land(&on_board, level, &mut cues);
                    }
                    flight.animator.land_progress(progress, &mut cues);
                } else if flight.progress.is_some() {
                    flight.animator.land_progress(1.0, &mut cues);
                    cues.push(ShipCue::Effect { fx: fx_defs::ROCKET_LAND, lift: 0.0, normal: None });
                } else {
                    flight.animator.update(dt, &mut cues);
                }
                flight.progress = landing;
                let animator = &flight.animator;
                let vm = &self.models[models::SHIP as usize];
                // The host's origin is the terrain contact point. Use the upright
                // rocket clip's endpoint, not the overturned tutorial wreck pose.
                let ground_anchor = *flight.ground_anchor.get_or_insert_with(|| {
                    let rest = vm.skel.pose_by(Mat4::IDENTITY, |j| {
                        let ctx = self.ship_contexts[j];
                        let (motion, frame) = if ctx == 0 { (11, 0.0) } else { animator.context(ctx) };
                        self.ship_anims.get(motion).map(|(_, a)| (a, frame))
                    });
                    let bottom = vm.batches.iter().flat_map(|b| &b.vertices)
                        .map(|v| rest.skin(v).0.y).fold(f32::INFINITY, f32::min);
                    rest.joints[2].w_axis.truncate() - Vec3::Y * bottom
                });
                let mut pose = vm.skel.pose_by(Mat4::IDENTITY, |j| {
                    let ctx = self.ship_contexts.get(j).copied().unwrap_or(0);
                    let (motion, frame) = if ctx == 0 && !animator.landing() {
                        (11, 0.0)
                    } else { animator.context(ctx) };
                    let (_, anim) = self.ship_anims.get(motion)?;
                    Some((anim, frame.clamp(0.0, anim.frames.saturating_sub(1) as f32)))
                });
                let offset = ground_anchor - pose.joints[2].w_axis.truncate();
                rebase_pose(&mut pose, offset);
                // The level lights' frames per material, and each part on board posed on its joint
                // of the ship (`PlayerState::renderParts`).
                let light_frames: Vec<f32> = self.ship_lights.iter().map(|l| l.map_or(0.0, |l| animator.light_frame(l))).collect();
                let mut part_joints: Vec<Option<Mat4>> = vec![None; self.parts.len()];
                let mut part_poses = Vec::new();
                for &part in &on_board {
                    let Some(Some(pv)) = self.parts.get(part as usize) else { continue };
                    let Some(ji) = vm.model.joint_index(&libpikmin_assets::ship::ship_joint(&pv.file)) else { continue };
                    let jm = pose.joints[ji];
                    part_joints[part as usize] = Some(jm);
                    let Some(pa) = animator.part(part as usize) else { continue };
                    let part_vm = &self.models[pv.model as usize];
                    let ppose = match pv.anims.as_ref() {
                        Some(set) => part_vm.skel.pose_by(jm, |k| {
                            let p = if pv.upper.get(k).copied().unwrap_or(false) { pa.upper.as_ref()? } else { &pa.lower };
                            let (_, a) = set.get(p.motion)?;
                            Some((a, p.frame.clamp(0.0, a.frames.saturating_sub(1) as f32)))
                        }),
                        None => part_vm.skel.pose(jm, None),
                    };
                    part_poses.push((part, pv.model, ppose));
                }
                let direction = o.goal_pos() - o.pos;
                let rot = Quat::from_rotation_y(direction.x.atan2(direction.z));
                let sphere = |id: &str| {
                    let c = vm.model.coll.iter().find(|c| c.id == id)?;
                    Some(o.pos + rot * pose.joints.get(c.joint)?.transform_point3(c.centre))
                };
                let fx_key = (6u64 << 48) | ((o.id.0 as u64) << 8);
                for cue in cues {
                    match cue {
                        ShipCue::Sound(name) => self.ship_sounds.push((o.id, name, true)),
                        ShipCue::StopSound(name) => self.ship_sounds.push((o.id, name, false)),
                        ShipCue::DemoSound { demo, cmd } => self.demo_sounds.push(Some((demo, cmd))),
                        ShipCue::EndDemoSound => self.demo_sounds.push(None),
                        ShipCue::StopAttached => {
                            flight.flames.clear();
                            if let Some(e) = self.effects.as_mut() { e.detach_where(|k| k & !255 == fx_key); }
                        }
                        ShipCue::Attached { fx, sphere: id, normal, slot } => {
                            if let (Some(at), Some(e)) = (sphere(id), self.effects.as_mut()) {
                                if let Some(g) = e.attach(fx_key | slot as u64, fx, at)
                                    && let Some(n) = normal { g.oriented_normal = rot * n; }
                                flight.flames.retain(|f| f.0 != slot);
                                flight.flames.push((slot, id));
                            }
                        }
                        ShipCue::Effect { fx, lift, normal } => {
                            if let Some(e) = self.effects.as_mut()
                                && let Some(g) = e.spawn(fx, o.pos + Vec3::Y * lift)
                                && let Some(n) = normal { g.oriented_normal = rot * n; }
                        }
                        ShipCue::JointEffect { fx, joint, offset, dir, normal } => {
                            if let (Some(m), Some(e)) = (pose.joints.get(joint), self.effects.as_mut())
                                && let Some(g) = e.spawn(fx, o.pos + rot * m.transform_point3(offset))
                            {
                                g.emit_dir = rot * dir;
                                if let Some(n) = normal { g.oriented_normal = rot * n; }
                            }
                        }
                        ShipCue::PartEffect { fx, part } => {
                            if let (Some(Some(jm)), Some(e)) = (part_joints.get(part), self.effects.as_mut()) {
                                e.spawn(fx, o.pos + rot * jm.w_axis.truncate());
                            }
                        }
                    }
                }
                if let Some(e) = self.effects.as_mut() {
                    for &(slot, id) in &flight.flames {
                        if let Some(at) = sphere(id) { e.move_attached(fx_key | slot as u64, at); }
                    }
                }
                let at = self.place(key(kinds::SHIP, i as u64), o.pos, tick, alpha);
                let lit = |m: usize| Inputs { frame: Some(light_frames.get(m).copied().unwrap_or(0.0)), ..Inputs::default() };
                self.push(key(kinds::SHIP, i as u64), kinds::SHIP, models::SHIP, at, rot, pose, &lit, 1.0, 0);
                for (part, model, ppose) in part_poses {
                    self.push(key(kinds::SHIP_PART, ((o.id.0 as u64) << 8) | part as u64), kinds::SHIP_PART, model, at, rot, ppose, &no_inputs, 1.0, 0);
                }
                continue;
            }
            self.onion_light(sim, i, o, dt);
            let Some((_, anim, _)) = self.animator.onion(&self.onion_anims, i, o, dt) else {
                continue;
            };
            let k = key(kinds::ONION, i as u64);
            live.insert(crate::animator::onion_key(i));
            let skel = &self.models[models::ONION as usize].skel;
            let mut pose = match anim {
                Some((clip, at)) if o.anim == libpikmin::onion::OnionAnim::Hover && o.flight_motion.is_none() && at > 252.0 =>
                    skel.pose_crossfade(clip, at, 200.0, (at - 252.0) / 8.0),
                _ => skel.pose(Mat4::IDENTITY, anim),
            };
            if o.anim == libpikmin::onion::OnionAnim::Hover {
                let rest = skel.pose(Mat4::IDENTITY, self.onion_anims.get(1).map(|(_, a)| (a, 0.0)));
                let offset = rest.joints[1].w_axis.truncate() - pose.joints[1].w_axis.truncate();
                rebase_pose(&mut pose, offset);
            }
            let frame = o.colors[0].index() as f32 * 2.0;
            let inputs = move |_: usize| Inputs { frame: Some(frame), ..Default::default() };
            let pos = self.place(k, o.pos, tick, alpha);
            self.push(k, kinds::ONION, models::ONION, pos, Quat::IDENTITY, pose, &inputs, 1.0, 0);
        }
        self.arrivals.clear();

        // Pellets and corpses: the numbered pellets, sized to the thing being carried
        // (corpses too: a host's enemies have no Pikmin model). The model goes by carriers,
        // which for a numbered pellet is its number.
        for pel in sim.pellets() {
            if pel.state == PelletState::Gone {
                continue;
            }
            let idx = match pel.desc.min_carriers {
                20.. => 3,
                10.. => 2,
                5.. => 1,
                _ => 0,
            };
            let model = models::PELLETS[idx];
            let vm = &self.models[model as usize];
            // Sized by the model's bottom circle ('cent', `Pellet::getBottomRadius`): a numbered
            // pellet ([`libpikmin::PelletDesc::numbered`]) draws at its config scale of 1.
            let bottom = vm.model.coll.iter().find(|c| c.id == "cent").map_or(vm.radius(), |c| c.radius);
            let s = pel.desc.radius / bottom * pel.scale;
            let lift = -vm.lo.y * s;
            let k = key(kinds::PELLET, pel.id.0 as u64);
            let (pos, rot) = self.place_rot(k, pel.pos + Vec3::Y * lift, Quat::from_rotation_y(pel.yaw), tick, alpha);
            let pose = self.models[model as usize].skel.pose(srt_matrix(Vec3::splat(s), Vec3::ZERO, Vec3::ZERO), None);
            let tint = pel.desc.color.map(piki_rgb).unwrap_or([230, 230, 230, 255]);
            let inputs = move |_: usize| Inputs { reg0: Some(tint), ..Default::default() };
            self.push(k, kinds::PELLET, model, pos, rot, pose, &inputs, 1.0, 0);
        }

        // Sprouts and flying seeds, with the leaf, bud or flower on joint 3.
        let mut live_sprouts = Vec::new();
        for sp in sim.sprouts() {
            live_sprouts.push(sp.id);
            live.insert(crate::animator::sprout_key(sp.id));
            let k = key(kinds::SPROUT, sp.id.0 as u64);
            let anim = self.animator.sprout(&self.pikihead_anims, sp, dt);
            let rot = if matches!(sp.state, SproutState::Flying) && sp.vel.length() > 0.0 {
                // Flying seeds point their Y axis against their velocity.
                let y = -sp.vel.normalize();
                let x = y.cross(Vec3::Z).normalize_or(Vec3::X);
                let z = x.cross(y).normalize();
                Quat::from_mat3(&Mat3::from_cols(x, y, z))
            } else {
                Quat::IDENTITY
            };
            let pose = self.models[models::PIKIHEAD as usize].skel.pose(Mat4::IDENTITY, anim);
            let (pos, rot) = self.place_rot(k, sp.pos, rot, tick, alpha);
            let head = pose.joints.get(3).copied();
            let tint = piki_rgb(sp.color);
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            self.push(k, kinds::SPROUT, models::PIKIHEAD, pos, rot, pose, &inputs, 1.0, 0);
            if let Some(j) = head {
                let model = models::HAPPA[sp.happa as usize];
                let pose = self.models[model as usize].skel.pose(j, None);
                let k = key(kinds::SPROUT_HAPPA, sp.id.0 as u64);
                self.push(k, kinds::SPROUT_HAPPA, model, pos, rot, pose, &no_inputs, 1.0, 0);
            }
        }
        self.animator.retain_sprouts(&live_sprouts);
        self.growth_effects(sim);

        // Pikmin, with the leaf, bud or flower on joint 6 (`ViewPiki::demoDraw`).
        for p in sim.pikis() {
            let Some(look) = piki_look(&params, p) else {
                continue;
            };
            live.insert(crate::animator::piki_key(p.id));
            let k = key(kinds::PIKI, p.id.0 as u64);
            let (pos, rot) = self.place_rot(k, p.pos, Quat::from_rotation_y(p.face), tick, alpha);
            // Frozen: the pose held where it was, the body paled toward ice.
            let frozen = p.frozen > 0.0;
            let anim = self.animator.piki(&self.piki_anims, p, &look, if frozen { 0.0 } else { dt });
            let model = models::PIKMIN[p.color.index()];
            let pose = self.models[model as usize].skel.pose(srt_matrix(look.scale, Vec3::ZERO, Vec3::ZERO), anim);
            let head = pose.joints.get(6).copied();
            let tint = if frozen { icy(piki_rgb(p.color)) } else { piki_rgb(p.color) };
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            let f = if frozen { flags::FROZEN } else { 0 };
            self.push(k, kinds::PIKI, model, pos, rot, pose, &inputs, look.fade, f);
            if let Some(j) = head
                && !matches!(p.state, PikiState::Pressed { .. })
            {
                let model = models::HAPPA[p.happa as usize];
                let pose = self.models[model as usize].skel.pose(j, None);
                let k = key(kinds::HAPPA, p.id.0 as u64);
                self.push(k, kinds::HAPPA, model, pos, rot, pose, &no_inputs, look.fade, 0);
            }
        }

        // Captains, and each one's cursor with the marker under it (`Navi::refresh`).
        for c in sim.captains() {
            live.insert(crate::animator::captain_key(c.id));
            let k = key(kinds::CAPTAIN, c.id.0 as u64);
            let (pos, rot) = self.place_rot(k, c.pos, Quat::from_rotation_y(c.face), tick, alpha);
            let anim = self.animator.captain(&self.piki_anims, &params, c, dt);
            let mut root = crate::animator::captain_root(&params, c);
            let fx_key = (8u64 << 48) | ((c.id.0 as u64) << 8);
            if let Some(&p) = self.materialize.get(&c.id.0) {
                // Beamed in: a thin tall figure that fills out, in the ship's healing effects
                // (`NaviUfoState`'s recover burst, the goal spot's sparkles).
                if let Some(fx) = self.effects.as_mut() {
                    if self.materializing.insert(c.id.0) {
                        fx.spawn(fx_defs::NAVI_RECOVER, pos);
                        fx.attach(fx_key, fx_defs::NAVI_SPARKLE, pos);
                        // The Dolphin's own doorstep rings (`UfoItem::setSpotActive`), on its goal
                        // spot nearest him.
                        let goal = sim
                            .onions()
                            .iter()
                            .filter(|o| o.ship.is_some())
                            .map(|o| o.goal_pos())
                            .min_by(|a, b| a.distance(c.pos).total_cmp(&b.distance(c.pos)));
                        if let Some(goal) = goal {
                            fx.attach(fx_key | 1, fx_defs::NAVI_RINGS, goal);
                        }
                    }
                    fx.move_attached(fx_key, pos);
                }
                let p = p.clamp(0.0, 1.0);
                let ease = p * p * (3.0 - 2.0 * p);
                let tall = ((p * 1.6).min(1.0)).powi(2);
                root *= Mat4::from_scale(Vec3::new(ease.max(0.02), tall.max(0.02), ease.max(0.02)));
            } else if self.materializing.remove(&c.id.0)
                && let Some(fx) = self.effects.as_mut()
            {
                fx.detach_where(|k| k & !255 == fx_key);
            }
            let pose = self.models[models::OLIMAR as usize].skel.pose(root, anim);
            let flash = c.damage_frames > 0.0 && (c.damage_frames as i32 / 2) % 2 == 0;
            let f = if flash { flags::FLASH } else { 0 };
            // The whistle's beam starts at the antenna (`Navi::demoDraw`'s 'ante' sphere).
            let antenna = self
                .ante
                .as_ref()
                .and_then(|a| Some(pos + rot * pose.joints.get(a.joint)?.transform_point3(a.centre)))
                .unwrap_or(pos + Vec3::Y * 30.0);
            self.whistles.push(WhistleFrame {
                captain: c.id,
                state: c.whistle,
                antenna,
                cursor: c.cursor_world,
                captain_pos: pos,
                radius: c.whistle_radius(&params),
            });
            self.push(k, kinds::CAPTAIN, models::OLIMAR, pos, rot, pose, &no_inputs, 1.0, f);
            if !self.show_cursors {
                continue;
            }

            // On the ground 1 above, Z pointing away from the captain. Locked on, it stands up
            // in front of the target's body instead, facing the captain.
            let kc = key(kinds::CURSOR, c.id.0 as u64);
            let (at, rot) = match c.lock_view {
                Some(lock) => {
                    let at = self.place(kc, lock.pos, tick, alpha);
                    let up = (lock.eye - at).normalize_or(Vec3::Y);
                    let side = if up.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
                    let x = side.cross(up).normalize_or(Vec3::X);
                    let z = x.cross(up);
                    // It turns slowly about the line to the captain so it reads as a lock.
                    let spin = Quat::from_rotation_y(self.time * LOCK_SPIN);
                    (at, Quat::from_mat3(&Mat3::from_cols(x, up, z)) * spin)
                }
                None => {
                    let at = self.place(kc, c.cursor_world.with_y(c.cursor_world.y + 1.0), tick, alpha);
                    let z = (at - pos).with_y(0.0).normalize_or(Vec3::Z);
                    let x = Vec3::Y.cross(z).normalize_or(Vec3::X);
                    (at, Quat::from_mat3(&Mat3::from_cols(x, Vec3::Y, z)))
                }
            };
            let time = self.time;
            let inputs = move |_: usize| Inputs { frame: Some(time * 30.0), ..Default::default() };
            let pose = self.models[models::CURSOR as usize].skel.pose(Mat4::IDENTITY, None);
            self.push(kc, kinds::CURSOR, models::CURSOR, at, rot, pose, &inputs, 1.0, 0);
            // The marker takes the next Pikmin to throw's colour (`findNextThrowPiki`: the
            // nearest squad Pikmin within 200), else white.
            let next = c.held_piki().map(|id| sim.piki(id).color).or_else(|| {
                sim.pikis()
                    .iter()
                    .filter(|p| p.captain == Some(c.id) && matches!(p.state, PikiState::Squad { .. }))
                    .map(|p| (p, (p.pos - c.pos).length()))
                    .filter(|(_, d)| *d < 200.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(p, _)| p.color)
            });
            let tint = next.map(piki_rgb).unwrap_or([255; 4]);
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            let pose = self.models[models::MARKER as usize].skel.pose(Mat4::IDENTITY, None);
            let km = key(kinds::MARKER, c.id.0 as u64);
            self.push(km, kinds::MARKER, models::MARKER, at, rot, pose, &inputs, 1.0, 0);
        }

        if let Some(fx) = self.effects.as_mut() {
            fx.update(&self.whistles, params.navi.whistle_hold_time, dt);
        }

        self.animator.retain_players(&live);
        let drawn: HashSet<u64> = self.draws.iter().map(|d| d.key).collect();
        self.interp.retain(|k, _| drawn.contains(k));
    }
}
