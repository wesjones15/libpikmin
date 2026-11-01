//! A sim posed for a host that draws with its own engine: the disc's models (loaded once, for
//! the host to build meshes and textures from) and, every frame, a draw list of posed and
//! CPU-skinned instances. The host only uploads vertices; which animation plays, where, and in
//! what colour is decided here, the way the viewer decides it.
//!
//! Instances are in sim units and axes. Each one has a position and rotation; its vertices are
//! in that local frame (scale already applied), so a host places one object per instance.

use std::collections::{HashMap, HashSet, VecDeque};

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

static TEKI_MODELS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the enemies' models are asked for ([`set_teki_models`]).
pub fn teki_models_enabled() -> bool {
    TEKI_MODELS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Host extension: load the enemies' models (Dwarf and Spotty Bulborbs, [`crate::teki`]) so the host can
/// draw its own AI's enemies with [`View::set_tekis`]. Off by default (each model's materials are baked
/// at load). Call before [`View::load`].
pub fn set_teki_models(on: bool) {
    TEKI_MODELS.store(on, std::sync::atomic::Ordering::Relaxed);
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
    /// An enemy the host asked for ([`super::View::set_tekis`]); its id is the host's.
    pub const TEKI: u8 = 12;
    pub const NECTAR_DROP: u8 = 13;
    pub const NECTAR_PUDDLE: u8 = 14;
}

/// Draw flags.
pub mod flags {
    /// Flashing from a hit: draw brighter.
    pub const FLASH: u8 = 1;
    /// Frozen ([`libpikmin::Sim::freeze_pikis_in_sphere`]): hosts may add an icy sheen. Its
    /// colour is already paled and its animation held.
    pub const FROZEN: u8 = 2;
    /// Host keeps this instance's previous mesh; this draw contains no vertices.
    pub const RETAIN_POSE: u8 = 4;
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

/// Most material variants (a pellet's colour, a light's blink frame, ...) one model bakes before its new ones
/// reuse its default. Per model, so the Dolphin's blinking lights (hundreds of frames) and the cursor's pulse
/// cannot use up the room a pellet's colour needs; a model's own materials are not counted either (the Ship
/// Repair view loads the ship's 30 parts and every enemy beside the usual models).
const MAX_VARIANTS_PER_MODEL: u32 = 128;
/// Most variants of every model together, against runaway growth.
const MAX_VARIANTS: usize = 2048;

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
    /// Per batch [`ViewModel::batch_light`], for a model that brings its own materials (the Pianta skin).
    pub lit: Option<Vec<u8>>,
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
            lit: None,
        })
    }

    /// A model the host's other library built (the Pianta skin): its own batches and baked textures, no PVW materials.
    pub fn external(batches: Vec<Batch>, lo: Vec3, hi: Vec3, batch_texture: Vec<i32>, batch_blend: Vec<u8>,
        batch_env: Vec<Option<Option<[[f32; 3]; 2]>>>, lit: Vec<u8>) -> Self {
        let model = Model::default();
        let images = tev::Images::new(&model);
        ViewModel {
            vertex_count: batches.iter().map(|b| b.vertices.len()).sum(),
            skel: Skeleton { parents: Vec::new(), bind: Vec::new(), inverse_bind: Vec::new(), envelopes: Vec::new() },
            model, batches, lo, hi, images, textured: Vec::new(), batch_texture, batch_blend, batch_env,
            lit: Some(lit),
        }
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
        if let Some(lit) = &self.lit {
            return lit.get(b).copied().unwrap_or(1);
        }
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
    versions: HashMap<i32, u32>,
    /// Textures baked with per-draw inputs (not a material's own), per model, against
    /// [`MAX_VARIANTS_PER_MODEL`] and, together, [`MAX_VARIANTS`].
    variants: HashMap<u32, u32>,
    variant_total: usize,
}

impl Textures {
    pub fn version(&self, id: u32) -> u32 { self.versions.get(&(id as i32)).copied().unwrap_or(0) }

    fn animated(&mut self, model: u32, vm: &ViewModel, material: usize, seconds: f32) -> i32 {
        let id = self.get(model, vm, material, Inputs::default());
        if id < 0 || !vm.model.materials[material].tex_datas.iter().any(|t| t.frames > 0 && t.anim_factor != 255) { return id; }
        let version = (seconds * 30.0).floor() as u32 + 1;
        if self.version(id as u32) != version {
            if let Some(baked) = tev::bake_at(&vm.model, material, Inputs::default(), &vm.images, seconds) {
                self.list[id as usize] = baked;
                self.versions.insert(id, version);
            }
        }
        id
    }
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
        // With the table full a variant is the material's own texture, which is always baked (counting every
        // texture here recursed without end once the models' own passed the cap).
        let variant = k.2 != tev::key(&vm.model, material, Inputs::default());
        if variant && (self.variants.get(&model).copied().unwrap_or(0) >= MAX_VARIANTS_PER_MODEL
            || self.variant_total >= MAX_VARIANTS)
        {
            return self.get(model, vm, material, Inputs::default());
        }
        let id = match tev::bake(&vm.model, material, inputs, &vm.images) {
            Some(b) => {
                if variant {
                    *self.variants.entry(model).or_insert(0) += 1;
                    self.variant_total += 1;
                }
                self.list.push(b);
                let id = self.list.len() as i32 - 1;
                if models::BEAMS.contains(&model) && vm.model.materials[material].tex_datas.iter().any(|t| t.frames > 0 && t.anim_factor != 255) {
                    self.versions.insert(id, 1);
                }
                id
            }
            None => -1,
        };
        self.ids.insert(k, id);
        id
    }
}

#[cfg(test)]
mod texture_motion_tests {
    use super::*;

    /// The Dolphin's lights and the cursor bake hundreds of variants as they animate; a pellet's colour
    /// is still its own texture after that (once it was the default's, so every pellet was blue).
    #[test]
    fn a_models_flood_of_variants_does_not_take_the_room_of_another_models() {
        let assets = Assets { root: Assets::default_dir() };
        if !assets.root.join(paths::ONION_BEAMS[0]).exists() { eprintln!("SKIP: Pikmin assets absent"); return; }
        let view = View::load(&assets).unwrap();
        let mut textures = Textures::default();
        let ship = &view.models[models::SHIP as usize];
        for f in 0..3000 {
            for m in 0..ship.model.materials.len() {
                textures.get(models::SHIP, ship, m, Inputs { frame: Some(f as f32 * 0.5), ..Default::default() });
            }
        }
        let cursor = &view.models[models::CURSOR as usize];
        for f in 0..3000 {
            textures.get(models::CURSOR, cursor, 0, Inputs { frame: Some(f as f32 * 0.5), ..Default::default() });
        }
        let pellet = &view.models[models::PELLETS[0] as usize];
        let own = textures.get(models::PELLETS[0], pellet, 0, Inputs::default());
        for colour in libpikmin::PikiColor::ALL {
            let tinted = textures.get(models::PELLETS[0], pellet, 0, Inputs { reg0: Some(piki_rgb(colour)), ..Default::default() });
            if colour != libpikmin::PikiColor::Blue {
                assert_ne!(tinted, own, "{colour:?} pellet fell back to the default texture");
            }
        }
    }

    #[test]
    fn onion_layers_move_at_disc_speeds_without_new_texture_slots() {
        let assets = Assets { root: Assets::default_dir() };
        if !assets.root.join(paths::ONION_BEAMS[0]).exists() { eprintln!("SKIP: Pikmin beam assets absent"); return; }
        for (i, path) in paths::ONION_BEAMS.iter().enumerate() {
            let model = ViewModel::load(&assets, path).unwrap();
            let mut textures = Textures::default();
            let id = textures.animated(models::BEAMS[i], &model, 1, 0.);
            let before = textures.list[id as usize].rgba.clone();
            let count = textures.list.len();
            textures.animated(models::BEAMS[i], &model, 1, 0.34);
            assert_ne!(before, textures.list[id as usize].rgba, "{path} layers froze");
            let version = textures.version(id as u32);
            textures.animated(models::BEAMS[i], &model, 1, 0.35);
            assert_eq!(version, textures.version(id as u32), "rebaked within the same game frame");
            for seconds in [1., 2., 5., 60.] { assert_eq!(id, textures.animated(models::BEAMS[i], &model, 1, seconds)); }
            assert_eq!(count, textures.list.len());
        }
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

/// A named joint of a posed body, for the host's attachment points on the Pianta and
/// Louie rigs (their joint counts differ, so names are the stable mapping).
fn named(rig: &libsms::Skeleton, joints: &[Mat4], name: &str) -> Option<Mat4> {
    rig.find(name).and_then(|j| joints.get(j)).copied()
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
    sucking: bool,
    seen: Vec<u32>,
    pending: VecDeque<u32>,
    repair: Option<(u32, f32)>,
    ground_anchor: Option<Vec3>,
    animator: libpikmin_assets::ship::ShipAnimator,
    progress: Option<f32>,
    flames: Vec<(u8, &'static str)>,
    trouble: bool,
    trouble_spark_time: f32,
}

/// A flying Onion's lean into its travel, like a helicopter: a small pitch toward the way it is
/// moving, eased in and out. Host extension: the disc's flight clips have no horizontal travel
/// at all (the Onion only ever rises or descends), so this has to come from the host's trajectory.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnionLean {
    pub quat: Quat,
}

impl OnionLean {
    /// Full lean, in radians (about 11 degrees).
    pub const MAX: f32 = 0.20;
    /// The speed (sim units a second) that reaches full lean; faster only holds it.
    pub const FULL_SPEED: f32 = 500.0;
    /// How long the lean takes to follow a change of speed or direction.
    const EASE: f32 = 0.18;

    /// Follows the Onion from `from` to `to` over `seconds`. `flying` off eases the lean back to
    /// upright, so a landed Onion stands straight. Returns the tilt to draw it with.
    pub fn advance(&mut self, from: Vec3, to: Vec3, seconds: f32, flying: bool) -> Quat {
        let target = if !flying || seconds <= 0.0 {
            Quat::IDENTITY
        } else {
            let travel = (to - from) / seconds;
            let flat = Vec3::new(travel.x, 0.0, travel.z);
            let speed = flat.length();
            if speed < 0.5 {
                Quat::IDENTITY
            } else {
                let axis = Vec3::Y.cross(flat / speed);
                Quat::from_axis_angle(axis, Self::MAX * (speed / Self::FULL_SPEED).clamp(0.0, 1.0))
            }
        };
        self.quat = self.quat.slerp(target, 1.0 - (-seconds / Self::EASE).exp());
        self.quat
    }
}

// Strip only cinematic travel; the clip still supplies every articulated pose. Hosts
// place the model along their collision-safe trajectory instead of adding both motions.
fn rebase_pose(pose: &mut libpikmin_assets::pose::Pose, offset: Vec3) {
    let translation = Mat4::from_translation(offset);
    pose.root = translation * pose.root;
    for matrix in pose.joints.iter_mut().chain(pose.envelopes.iter_mut()) { *matrix = translation * *matrix; }
}

/// A loaded enemy kind: its facts and animations (the model is in [`View::models`]).
pub struct TekiView {
    pub info: crate::teki::TekiInfo,
    anims: AnimSet,
    base: &'static str,
    /// Per motion: the frames where its animation plays a sound from the kind's table, and which one
    /// (`PaniAnimator::checkEventKeys`, event 0).
    sound_keys: Vec<Vec<(f32, u8)>>,
}

/// A creature id for the audio's table sounds and positions: the host's enemies are numbered apart from
/// the sim's creatures.
pub fn teki_creature_id(id: u32) -> libpikmin::host::CreatureId {
    (1u64 << 40) | id as u64
}

pub struct View {
    /// Enemy kinds that loaded ([`set_teki_models`]), by [`crate::teki::TekiInfo::kind`] order.
    pub tekis: Vec<TekiView>,
    /// Pikmin 2's enemy kinds that loaded ([`View::load_p2_tekis`]).
    p2_tekis: Vec<std::sync::Arc<crate::p2enemy::P2Teki>>,
    teki_input: Vec<crate::teki::TekiInstance>,
    /// The Long Legs' legs as the core has them, by the instance id (`View::set_teki_legs`).
    teki_legs: HashMap<u32, libpikmin_teki::legs::LegsView>,
    /// The next body pushed keeps its place in the model's space instead of standing on the ground (a body whose legs set its height).
    no_lift: bool,
    /// Each enemy's motion and frame at the last update, to find the sound keys it passed.
    teki_played: HashMap<u32, (u32, f32)>,
    /// Sounds from the enemies' animations this update: (creature id, table's first `SE_*`, index), and
    /// where each enemy is, for [`libpikmin_present::audio::Creatures`]. Hosts hand them to the audio.
    pub teki_sounds: Vec<(libpikmin::host::CreatureId, &'static str, u8)>,
    pub teki_positions: Vec<(libpikmin::host::CreatureId, Vec3)>,
    /// Opt-in distant skinning budget; hosts must understand RETAIN_POSE first.
    pub pose_budget: Option<(Vec3, f32)>,
    render_frame: u64,
    posed_models: HashMap<u64, u32>,
    /// Opt in to the original wreck and progressively repaired root poses.
    pub repair_mode: bool,
    /// Host-owned victory presentation; never changes simulation or objective state.
    pub celebrating: bool,
    pub models: Vec<ViewModel>,
    piki_anims: AnimSet,
    /// Olimar's motion from the bad ending that feeds him into the Onion, if extracted (Onion Ride's boarding).
    boarding: Option<libpikmin_assets::Anim>,
    /// Captains playing it: the frame and the Onion's foot it is relative to.
    boarding_clips: HashMap<u32, (f32, Vec3)>,
    onion_anims: AnimSet,
    nectar_models: [u32;2],
    nectar_anims: [AnimSet;2],
    boot_sounds: [(Vec<(f32, u16)>, f32); 3],
    boot_progress: HashMap<u32, f32>,
    ship_anims: AnimSet,
    ship_mgr: libpikmin_assets::AnimMgr,
    ship_contexts: Vec<usize>,
    /// Which of the ship's level lights animates each of its materials (`LightAnimator`).
    ship_lights: Vec<Option<usize>>,
    /// Each of the 30 ship parts' model, animations and the joints its upper animator drives, when loaded.
    parts: Vec<Option<PartView>>,
    ships: HashMap<u32, ShipFlight>,
    pub onion_ground: HashMap<u32, f32>,
    /// Host-controlled hovering Onion menu beams (pellet suction supplies its own beam).
    pub onion_menu_beams: HashSet<u32>,
    onion_lift_smoke: HashSet<u32>,
    /// Per flying Onion: the tick and position its lean was last measured from, and the lean.
    onion_lean: HashMap<u32, (u64, Vec3, OnionLean)>,
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
    pub p2_effects: Option<crate::p2effects::P2Effects>,
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
    /// Host healing at the captain, independent of Dolphin arrival/transport.
    healing: HashMap<u32, (f32, u32)>,
    healing_active: HashSet<u32>,
    /// Onions whose beacon rings are on (`GoalItem::setSpotActive`).
    spots: HashSet<u32>,
    /// Ships whose goal spot rings are on (`UfoItem::setSpotActive`).
    ship_spots: HashSet<u32>,
    /// Onions a pellet just reached, for their `GoalEffect`.
    arrivals: Vec<libpikmin::onion::OnionId>,
    /// Growth and deflowering seen in events, for their effects once the sim's positions are
    /// to hand.
    grown: Vec<Grown>,
    /// The stage each sprout already sparkled for (`PikiHeadAI::WaitExec` once per stage).
    sparkled: HashMap<libpikmin::SproutId, libpikmin::Happa>,
    /// Each captain's body attachment points as last drawn, in that body's model space, for
    /// a host mounting props (item displays) on him. Same ids as `libsms_view`'s
    /// `sms_view_anchor`: 0 head, 1 chest, 2 pelvis, 3/4 shoulders, 5/6 elbows, 7/8 hands,
    /// 9/10 legs, 11/12 knees, 13/14 feet.
    captain_anchors: HashMap<u32, [Option<glam::Mat4>; 15]>,
    /// The raw attachment points for the captain a `push_body`/`push_hsd` call is about to
    /// draw, in his body's own units; the push applies its scale and ground lift and stores
    /// them in `captain_anchors`.
    pending_anchors: Option<[Option<Mat4>; 15]>,
    /// The Pianta body some captains wear ([`View::load_pianta`]) and who wears it.
    pub(crate) pianta: Option<crate::pianta::PiantaSkin>,
    /// The Louie body some captains wear ([`View::load_louie`]).
    pub(crate) louie: Option<crate::louie::LouieSkin>,
    /// Louie's navi light (a stand-in for the JPA2 effect) and where it burns this frame: (feet, height).
    navi_light: Option<crate::navilight::NaviLight>,
    /// Louie's ship, drawn in place of the Dolphin for the Onions listed in `hocotate_ships` ([`View::set_ship_hocotate`]).
    hocotate: Option<std::sync::Arc<crate::hocotate::HocotateSkin>>,
    hocotate_ships: HashSet<u32>,
    /// The Dolphin's height standing upright, which the Hocotate Ship is sized to.
    dolphin_height: Option<f32>,
    light_spots: Vec<(Vec3, f32)>,
    /// Pikmin 2's treasures ([`View::load_treasures`]): their bodies, the stand-in effects, what the view remembers of
    /// each treasure pellet, the sounds due on the host's Pikmin 2 output, and what sparkles or bursts this frame.
    treasures: Option<std::sync::Arc<crate::treasure::TreasureSet>>,
    treasure_fx: Option<crate::treasure::TreasureFx>,
    treasure_tracks: crate::treasure::Tracks,
    treasure_cues: Vec<crate::treasure::Cue>,
    treasure_bursts: Vec<(Vec3, f32, crate::treasure::Burst, f32)>,
    treasure_spots: Vec<(Vec3, f32, u32)>,
    /// Candypop Buds ([`View::load_bulbs`]).
    bulbs: Option<std::sync::Arc<crate::bulb::BulbSet>>,
    /// The buds' stand-in effects, what the view remembers of each bud, the bursts running and the open buds that twinkle.
    bulb_fx: Option<crate::bulb::BulbFx>,
    bulb_tracks: crate::bulb::Tracks,
    bulb_bursts: Vec<(Vec3, libpikmin::PikiColor, crate::bulb::Burst, f32)>,
    bulb_spots: Vec<(Vec3, libpikmin::PikiColor, u32)>,
    /// Purple and White Pikmin (Pikmin 2, [`View::load_pikmin2`]).
    pub(crate) pikmin2: Option<std::sync::Arc<crate::pikmin2::Pikmin2Skin>>,
    /// Pikachu and his Ice Climbers ([`View::load_pikachu`]).
    pub(crate) pikachu: Option<std::sync::Arc<crate::pikachu::PikachuSkin>>,
    captain_skins: HashMap<u32, u8>,
    /// Cries a Pianta captain made since the host last took them ([`View::take_pianta_voices`]).
    pianta_voices: Vec<(u32, u8, crate::pianta::Voice)>,
}

enum Grown {
    Plucked(libpikmin::PikiId),
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
/// The longest a hovering Onion's beam is stretched to reach the ground, in sim units (about 32 m).
const BEAM_MAX_DROP: f32 = 800.0;
/// A flying Onion this close to its ground (sim units, about 5.6 m) is *descended*: its lift's
/// ground cloud runs, as `GoalItem` does while the rocket is near the floor.
const ONION_DESCENDED: f32 = 140.0;
/// Height (sim units, about 2.4 m) under which a flying Onion also puts its beam down, so the
/// Onion menu's descent has it out by the time the Onion settles.
const ONION_BEAM_HEIGHT: f32 = 60.0;
/// Height over which the lifted ground cloud fades out, in sim units.
const ONION_CLOUD_FADE: f32 = 180.0;
/// `GoalEffect::invoke`: no more gravity from this frame of the suck effect.
const SUCK_GRAVITY_FRAMES: f32 = 50.0;

impl View {
    pub fn pending_repairs(&self) -> usize {
        self.ships.values().map(|s| s.pending.len() + usize::from(s.repair.is_some())).sum()
    }
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
        let mut tekis = Vec::new();
        if TEKI_MODELS.load(std::sync::atomic::Ordering::Relaxed) {
            let pellets = assets.read("parms/pelMgr.bin").ok()
                .and_then(|d| libpikmin::pellet_config::parse_pellet_configs(&d).ok()).unwrap_or_default();
            for kind in (0..crate::teki::KINDS as u8).chain([crate::teki::P1_FLINT]) {
                let Some([model_path, anm, key, _]) = crate::teki::paths(kind) else { continue };
                let loaded = (|| {
                    let vm = ViewModel::load(assets, &model_path).ok()?;
                    let info = crate::teki::TekiInfo::load(assets, kind, models.len() as u32, &pellets).ok()?;
                    let anims = AnimSet::load(assets, &key, &anm).ok()?;
                    let sound_keys = anims.mgr.infos.iter()
                        .map(|i| i.event_keys.iter().filter(|e| e.event == 0).map(|e| (e.frame as f32, e.value)).collect())
                        .collect();
                    let base = crate::teki::definition(kind)?.sound_base;
                    Some((vm, TekiView { info, anims, base, sound_keys }))
                })();
                if let Some((vm, teki)) = loaded {
                    models.push(vm);
                    tekis.push(teki);
                }
            }
        }
        // ItemMgr::initialise registers these exact MizuItem shapes. Append them
        // after optional ship/enemy models so the existing fixed model IDs stay put.
        let nectar_models = [models.len() as u32, models.len() as u32 + 1];
        models.push(ViewModel::load(assets, paths::FALL_WATER)?);
        models.push(ViewModel::load(assets, paths::WATER)?);
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
        // Reserve bounded palette variants up front so repeated revivals cannot exhaust
        // the shared texture cache and fall back to the default blue Onion.
        let onion = &models[models::ONION as usize];
        for colour in 0..3 {
            for step in 0..=4 {
                for b in &onion.batches {
                    textures.get(models::ONION, onion, b.material, Inputs {
                        frame: Some(colour as f32 * 2.0 + step as f32 * 0.25), ..Default::default()
                    });
                }
            }
        }
        let ship_contexts = models[models::SHIP as usize].skel.override_map(&libpikmin_assets::ship::CONTEXT_JOINTS);
        Ok(View {
            tekis,
            p2_tekis: Vec::new(),
            teki_input: Vec::new(),
            teki_legs: HashMap::new(),
            no_lift: false,
            teki_played: HashMap::new(),
            teki_sounds: Vec::new(),
            teki_positions: Vec::new(),
            repair_mode: false,
            celebrating: false,
            models,
            piki_anims: AnimSet::load(assets, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE)?,
            boarding: assets.read(libpikmin_assets::extract::BOARDING_CLIP).ok()
                .and_then(|d| libpikmin_assets::anim::parse_bundle(&d).ok()).and_then(|v| v.into_iter().next()),
            boarding_clips: HashMap::new(),
            onion_anims: AnimSet::load(assets, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE)?,
            nectar_models,
            nectar_anims: [AnimSet::load(assets, paths::FALL_WATER_ANIM_MGR, paths::FALL_WATER_BUNDLE)?, AnimSet::load(assets, paths::WATER_ANIM_MGR, paths::WATER_BUNDLE)?],
            boot_sounds: [
                libpikmin_assets::sound::tables::onion_boot_commands(assets, false)?,
                libpikmin_assets::sound::tables::onion_boot_commands(assets, true)?,
                libpikmin_assets::sound::tables::onion_boot_commands(assets, false)?,
            ],
            boot_progress: HashMap::new(),
            ship_anims: AnimSet::load(assets, libpikmin_assets::ship::SHIP_ANIM_MGR, libpikmin_assets::ship::SHIP_BUNDLE)?,
            ship_mgr: libpikmin_assets::AnimMgr::parse(&assets.read(libpikmin_assets::ship::SHIP_ANIM_MGR)?)?,
            ship_contexts,
            ship_lights,
            parts,
            ships: HashMap::new(),
            onion_ground: HashMap::new(),
            onion_menu_beams: HashSet::new(),
            onion_lift_smoke: HashSet::new(),
            onion_lean: HashMap::new(),
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
            pose_budget: None,
            render_frame: 0,
            posed_models: HashMap::new(),
            textures,
            effects: Effects::load(assets),
            particles: Flat::default(),
            p2_effects: None,
            ante,
            whistles: Vec::new(),
            interp: HashMap::new(),
            time: 0.0,
            show_cursors: true,
            cones: HashMap::new(),
            materialize: HashMap::new(),
            materializing: HashSet::new(),
            healing: HashMap::new(),
            healing_active: HashSet::new(),
            spots: HashSet::new(),
            ship_spots: HashSet::new(),
            arrivals: Vec::new(),
            grown: Vec::new(),
            sparkled: HashMap::new(),
            pianta: None,
            louie: None,
            navi_light: None,
            hocotate: None,
            hocotate_ships: HashSet::new(),
            dolphin_height: None,
            light_spots: Vec::new(),
            treasures: None,
            treasure_fx: None,
            treasure_tracks: HashMap::new(),
            treasure_cues: Vec::new(),
            treasure_bursts: Vec::new(),
            treasure_spots: Vec::new(),
            bulbs: None,
            bulb_fx: None,
            bulb_tracks: HashMap::new(),
            bulb_bursts: Vec::new(),
            bulb_spots: Vec::new(),
            pikmin2: None,
            pikachu: None,
            captain_skins: HashMap::new(),
            captain_anchors: HashMap::new(),
            pending_anchors: None,
            pianta_voices: Vec::new(),
        })
    }

    pub fn on_event(&mut self, e: &Event) {
        self.animator.on_event(e);
        // A Pianta or Louie captain cries out with his own voice where Olimar would with his.
        {
            use crate::pianta::Voice;
            let cry = match *e {
                Event::CaptainJumped { captain, .. } => Some((captain, Voice::Jump)),
                Event::Thrown { captain, .. } => Some((captain, Voice::Throw)),
                Event::CaptainHurt { captain, .. } => Some((captain, Voice::Hurt)),
                Event::CaptainDown { captain } => Some((captain, Voice::Down)),
                Event::Whistle { captain, .. } => Some((captain, Voice::Whistle)),
                Event::Dismissed { captain } => Some((captain, Voice::Dismiss)),
                Event::Punched { captain, .. } => Some((captain, Voice::Punch)),
                _ => None,
            };
            if let Some((captain, voice)) = cry {
                match self.captain_skins.get(&captain.0).copied() {
                    Some(crate::pianta::SKIN_PIANTA) if self.pianta.is_some() => {
                        self.pianta_voices.push((captain.0, crate::pianta::SKIN_PIANTA, voice));
                    }
                    Some(crate::louie::SKIN_LOUIE) if self.louie.is_some() && crate::louie::voice_id(voice).is_some() => {
                        self.pianta_voices.push((captain.0, crate::louie::SKIN_LOUIE, voice));
                    }
                    _ => {}
                }
            }
        }
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
            Event::PluckBurst { piki } => self.grown.push(Grown::Plucked(*piki)),
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
                Grown::Plucked(id) => {
                    for def in fx_defs::PLUCK_DIRT { fx.spawn(def, sim.piki(id).pos); }
                }
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
        // PelletGoalState::init uses the Onion beam while an airborne Onion ingests a pellet.
        let ingesting = sim.pellets().iter().any(|p| matches!(p.state,
            libpikmin::PelletState::Sucking { onion, .. } if onion == o.id));
        // Host extension: a flying Onion that has come down to its ground (the Onion menu's
        // descent, Pikmin climbing aboard) keeps its beam out, as a landed one does.
        let ground = self.onion_ground.get(&o.id.0).copied();
        let airborne = !grounded;
        let height = ground.map_or(0.0, |g| (o.pos.y - g).max(0.0));
        let beam_active = grounded || ingesting || self.onion_menu_beams.contains(&o.id.0)
            || (airborne && ground.is_some() && height <= ONION_BEAM_HEIGHT);
        let cone = self.cones.entry(o.id.0).or_insert(if beam_active { 1.0 } else { 0.0 });
        let step = dt / CONE_TIME;
        *cone = if beam_active { (*cone + step).min(1.0) } else { (*cone - step).max(0.0) };
        let width = *cone;
        if width > 0.0 && o.anim != OnionAnim::Away {
            let model = models::BEAMS[colour];
            let (lo, hi) = (self.models[model as usize].lo.y, self.models[model as usize].hi.y);
            // A flying Onion hovers above the ground, so its beam would hang in the air. Host extension:
            // the beam is stretched down to the ground below it, its top staying where it was.
            let drop = if airborne && hi > lo {
                ground.map_or(0.0, |g| (o.pos.y - g).clamp(0.0, BEAM_MAX_DROP))
            } else { 0.0 };
            let (height, base) = if drop > 1.0 {
                let height = (drop + hi * CONE_SCALE) / (hi - lo);
                (height, o.pos.y - drop - lo * height)
            } else { (CONE_SCALE, o.pos.y) };
            let skel = &self.models[model as usize].skel;
            let root = Mat4::from_scale(Vec3::new(CONE_SCALE * width, height, CONE_SCALE * width));
            let pose = skel.pose(root, None);
            let k = key(kinds::BEAM, i as u64);
            self.push(k, kinds::BEAM, model, Vec3::new(o.pos.x, base, o.pos.z), Quat::IDENTITY, pose, &|_| Inputs::default(), 1.0, 0);
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

    /// The enemies to draw at the next [`View::update`] (and only that one): the host's own, posed
    /// by the animation and frame it names. Kinds that didn't load are skipped.
    pub fn set_tekis(&mut self, list: &[crate::teki::TekiInstance]) {
        self.teki_input.clear();
        self.teki_input.extend_from_slice(list);
    }

    /// The Long Legs' legs (`libpikmin_teki::Teki::legs`, `Legs::view`) for the instances of [`View::set_tekis`], by instance id, replacing
    /// any given before: the body is drawn at their trace point facing their way with its legs folded onto their feet.
    pub fn set_teki_legs(&mut self, list: &[(u32, libpikmin_teki::legs::LegsView)]) {
        self.teki_legs.clear();
        self.teki_legs.extend(list.iter().copied());
    }

    /// Plays `EffectMgr`'s effect `id` (`EFF_*`, [`libpikmin_assets::effect_table`]) once at `pos`: the
    /// enemies' death smoke and souls, a host's attack effects. False if the effect or its files aren't there.
    pub fn spawn_effect(&mut self, id: usize, pos: Vec3) -> bool {
        if id&0x8000!=0 {if let Some(fx)=self.p2_effects.as_mut() {fx.emit((id&0x7fff) as u16,Mat4::from_translation(pos),1.0);return true;}return false;}
        let Some(def) = libpikmin_assets::effect_table::by_id(id) else { return false };
        self.effects.as_mut().is_some_and(|e| e.spawn(def, pos).is_some())
    }

    /// As [`View::spawn_effect`], with the direction it emits in (a flame from a mouth, wind).
    pub fn spawn_effect_dir(&mut self, id: usize, pos: Vec3, dir: Vec3) -> bool {
        if id&0x8000!=0 {if let Some(fx)=self.p2_effects.as_mut() {fx.emit((id&0x7fff) as u16,Mat4::from_translation(pos)*Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y,dir.normalize_or(Vec3::Y))),1.0);return true;}return false;}
        let Some(def) = libpikmin_assets::effect_table::by_id(id) else { return false };
        let Some(e) = self.effects.as_mut() else { return false };
        match e.spawn(def, pos) {
            Some(g) => {
                if dir.length() > 1e-4 {
                    g.emit_dir = dir.normalize();
                }
                true
            }
            None => false,
        }
    }

    /// The loaded kind `kind` ([`crate::teki::DWARF`]...), if its files were on the disc.
    pub fn teki(&self, kind: u8) -> Option<&TekiView> {
        self.tekis.iter().find(|t| t.info.kind == kind)
    }

    /// What the disc says about enemy kind `kind`, Pikmin 1's or Pikmin 2's, if it loaded.
    pub fn teki_info(&self, kind: u8) -> Option<&crate::teki::TekiInfo> {
        self.teki(kind).map(|t| &t.info).or_else(|| self.p2_tekis.iter().find(|t| t.info.kind == kind).map(|t| &t.info))
    }

    /// Loads Pikmin 2's enemies (`libpikmin_teki::p2`) from the cache `dir` (`libsms::extract::p2_enemy`, the folder holding
    /// `enemy/`), appending one model per kind to the table; returns how many loaded. Call before the host reads the table.
    pub fn load_p2_tekis(&mut self, dir: &std::path::Path) -> usize {
        if !self.p2_tekis.is_empty() { return self.p2_tekis.len(); }
        let loaded = crate::p2enemy::load(dir, self.models.len() as u32, self.textures.list.len() as i32);
        for t in &loaded {
            let m = &t.body.model;
            let (lo, hi) = (m.lo * t.info.scale, m.hi * t.info.scale);
            let tex_base = t.tex_base;
            self.textures.list.extend(t.textures());
            self.models.push(ViewModel::external(
                t.batches(), lo, hi,
                m.batch_texture.iter().map(|&b| if b >= 0 { b + tex_base } else { -1 }).collect(),
                m.batch_blend.clone(),
                m.batch_env.clone(),
                m.batch_lit.iter().map(|l| l & 3).collect(),
            ));
        }
        self.p2_effects = crate::p2effects::P2Effects::load(dir).ok();
        self.p2_tekis = loaded;
        self.p2_tekis.len()
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
        if let Some(fx)=self.p2_effects.as_mut() { fx.draw(&mut self.particles,right,up); }
        // Louie's navi light over each captain wearing him.
        if let Some(mut light) = self.navi_light.take() {
            for &(feet, height) in &self.light_spots {
                light.draw(&mut self.particles, feet, height, self.time, right, up);
            }
            self.navi_light = Some(light);
        }
        // Candypop Buds: a twinkle at the mouth of each open one, and the bursts of the moment.
        if let Some(mut fx) = self.bulb_fx.take() {
            for &(feet, colour, key) in &self.bulb_spots {
                fx.twinkle(&mut self.particles, feet, colour, key, self.time, right, up);
            }
            for &(feet, colour, kind, born) in &self.bulb_bursts {
                fx.burst(&mut self.particles, kind, feet, colour, self.time - born, right, up);
            }
            self.bulb_fx = Some(fx);
        }
        // Treasures: a twinkle around each one on the map, and the burst of one appearing or being taken in.
        if let Some(mut fx) = self.treasure_fx.take() {
            for &(feet, radius, key) in &self.treasure_spots {
                fx.sparkle(&mut self.particles, feet, radius, key, self.time, right, up);
            }
            for &(at, radius, kind, born) in &self.treasure_bursts {
                fx.burst(&mut self.particles, kind, at, radius, self.time - born, right, up);
            }
            self.treasure_fx = Some(fx);
        }
    }

    /// Loads all 188 of Pikmin 2's treasures from the cache `dir` (`libsms::extract::p2_treasure`, in `dir/treasure`),
    /// appending 188 models to the table in `otakara_config.txt` order; returns the first one's index. Call before the
    /// host reads the table. Treasure pellets ([`libpikmin::TREASURE_PART_BASE`] and up) are then drawn with them,
    /// sparkle, and make Pikmin 2's sounds ([`View::take_treasure_cues`]).
    pub fn load_treasures(&mut self, dir: &std::path::Path) -> Result<u32, String> {
        if let Some(t) = &self.treasures { return Ok(t.bodies.first().map_or(0, |b| b.model)); }
        let first = self.models.len() as u32;
        let tex_base = self.textures.list.len() as i32;
        let set = crate::treasure::TreasureSet::load(dir, first, tex_base)?;
        for (i, b) in set.bodies.iter().enumerate() {
            let m = &b.treasure.model;
            let (lo, hi) = b.treasure.bounds();
            self.textures.list.extend(set.textures(i));
            self.models.push(ViewModel::external(
                set.batches(i), lo, hi,
                m.batch_texture.iter().map(|&t| if t >= 0 { t + b.tex_base } else { -1 }).collect(),
                m.batch_blend.clone(),
                m.batch_env.clone(),
                m.batch_lit.iter().map(|l| l & 3).collect(),
            ));
        }
        match crate::treasure::TreasureFx::load(dir) {
            Ok(fx) => self.treasure_fx = Some(fx),
            Err(e) => eprintln!("Treasure sparkles: {e}"),
        }
        self.treasures = Some(std::sync::Arc::new(set));
        Ok(first)
    }

    /// Loads Pikmin 2's Candypop Bud from the cache `dir` (the folder holding `enemy/`, `libsms::extract::p2_enemy`) in each
    /// of the five colours, appending five models to the table (Blue, Red, Yellow, Purple, White); returns the first one's
    /// index. Call before the host reads the table. The sim's buds ([`libpikmin::Sim::add_bulb`]) are then drawn.
    pub fn load_bulbs(&mut self, dir: &std::path::Path) -> Result<u32, String> {
        if let Some(b) = &self.bulbs { return Ok(b.bodies.first().map_or(0, |b| b.model)); }
        let first = self.models.len() as u32;
        let tex_base = self.textures.list.len() as i32;
        let set = crate::bulb::BulbSet::load(dir, first, tex_base)?;
        for (i, b) in set.bodies.iter().enumerate() {
            let m = &b.body.model;
            self.textures.list.extend(set.textures(i));
            self.models.push(ViewModel::external(
                set.batches(i), m.lo, m.hi,
                m.batch_texture.iter().map(|&t| if t >= 0 { t + b.tex_base } else { -1 }).collect(),
                m.batch_blend.clone(),
                m.batch_env.clone(),
                m.batch_lit.iter().map(|l| l & 3).collect(),
            ));
        }
        match crate::bulb::BulbFx::load(dir) {
            Ok(fx) => self.bulb_fx = Some(fx),
            Err(e) => eprintln!("Candypop Bud effects: {e}"),
        }
        self.bulbs = Some(std::sync::Arc::new(set));
        Ok(first)
    }

    /// The sounds treasures made since the last call (Pikmin 2's `SoundID.h` ids at places in sim units), for the host's
    /// Pikmin 2 sound output.
    pub fn take_treasure_cues(&mut self) -> Vec<crate::treasure::Cue> {
        std::mem::take(&mut self.treasure_cues)
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
        let retain_pose = matches!(kind, kinds::PIKI | kinds::HAPPA)
            && self.posed_models.get(&k) == Some(&model)
            && self.pose_budget.is_some_and(|(eye, far)| pos.distance_squared(eye) > far * far)
            && (self.render_frame + (k & 0xffff_ffff)) % 3 != 0;
        let first_vertex = self.positions.len() as u32;
        let first_color = self.colors.len() as u32;
        for (bi, b) in vm.batches.iter().enumerate() {
            let env = vm.batch_env.get(bi).copied().flatten();
            for v in &b.vertices {
                if retain_pose { break; }
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
            let id = if kind == kinds::BEAM {
                self.textures.animated(model, vm, b.material, self.time)
            } else { self.textures.get(model, vm, b.material, inp) };
            let mut c = if id >= 0 { [1.0; 4] } else { tev::flat_color(&vm.model, b.material, inp, &vm.images) };
            c[3] *= alpha;
            self.colors.push(c);
            self.texture_ids.push(id);
        }
        let flags = flags | if retain_pose { flags::RETAIN_POSE } else { 0 };
        if !retain_pose { self.posed_models.insert(k, model); }
        self.draws.push(Draw { key: k, kind, flags, model, pos, rot, first_vertex, first_color, alpha });
    }

    /// Loads the Pianta body some captains can wear from the Sunshine assets folder `dir` (the
    /// standard extraction has it); returns its model index, which is appended to the model table, so
    /// call it before the host reads the table. `female` and `colour` pick the Pianta.
    pub fn load_pianta(&mut self, dir: &std::path::Path, female: bool, colour: usize) -> Result<u32, String> {
        if let Some(p) = &self.pianta { return Ok(p.model); }
        let model = self.models.len() as u32;
        let tex_base = self.textures.list.len() as i32;
        let skin = crate::pianta::PiantaSkin::load(dir, &self.models[models::OLIMAR as usize].model, model, tex_base, female, colour)?;
        let m = &skin.body.model;
        let (lo, hi) = skin.bounds();
        let textures = skin.textures();
        self.textures.list.extend(textures);
        self.models.push(ViewModel::external(
            skin.batches(), lo, hi,
            m.batch_texture.iter().map(|&t| if t >= 0 { t + tex_base } else { -1 }).collect(),
            m.batch_blend.clone(),
            m.batch_env.clone(),
            m.batch_lit.iter().map(|l| l & 3).collect(),
        ));
        self.pianta = Some(skin);
        Ok(model)
    }

    /// A body attachment point as this captain's last draw posed it, in his model's space:
    /// 0 head, 1 chest, 2 pelvis, 3/4 shoulders, 5/6 elbows, 7/8 hands, 9/10 legs,
    /// 11/12 knees, 13/14 feet. `None` when the skin has no such part.
    pub fn captain_anchor(&self, captain: u32, id: usize) -> Option<Mat4> {
        self.captain_anchors.get(&captain).and_then(|a| a.get(id)).copied().flatten()
    }

    /// Who wears which body ([`crate::pianta::SKIN_OLIMAR`] or [`crate::pianta::SKIN_PIANTA`]); anything
    /// unloaded draws Olimar.
    pub fn set_captain_skin(&mut self, captain: u32, skin: u8) {
        if self.captain_skins.insert(captain, skin) != Some(skin)
            && let Some(pianta) = self.pianta.as_mut()
        {
            pianta.reset_pose(captain);
        }
    }

    /// The captains wearing Louie now.
    pub fn louie_captains(&self) -> HashSet<u32> {
        if self.louie.is_none() { return HashSet::new(); }
        self.captain_skins.iter().filter(|(_, s)| **s == crate::louie::SKIN_LOUIE).map(|(c, _)| *c).collect()
    }

    /// The captains wearing the Pianta now.
    pub fn pianta_captains(&self) -> HashSet<u32> {
        if self.pianta.is_none() { return HashSet::new(); }
        self.captain_skins.iter().filter(|(_, s)| **s == crate::pianta::SKIN_PIANTA).map(|(c, _)| *c).collect()
    }

    /// The sounds the Pianta's clips triggered since the last call (his footsteps): (captain, `JAISoundID`).
    pub fn take_pianta_cues(&mut self) -> Vec<(u32, u32)> {
        self.pianta.as_mut().map(|p| std::mem::take(&mut p.cues)).unwrap_or_default()
    }

    /// The cries Pianta captains made since the last call, for the host's sound system.
    pub fn take_pianta_voices(&mut self) -> Vec<(u32, u8, crate::pianta::Voice)> {
        std::mem::take(&mut self.pianta_voices)
    }

    /// How tall the Pianta stands, to the top of his head, in sim units.
    pub fn set_pianta_height(&mut self, units: f32) {
        if let Some(p) = self.pianta.as_mut() { p.set_height(units); }
    }

    /// Skins the Pianta from `joints` (his own units) with `root` over his scale, lowest vertex on the
    /// ground, and queues him as a captain draw.
    fn push_pianta(&mut self, k: u64, pos: Vec3, rot: Quat, root: Mat4, joints: &[Mat4], anchors: &[Option<Mat4>; 15], alpha: f32, flags: u8) {
        let Some(skin) = self.pianta.take() else { return };
        self.pending_anchors = Some(*anchors);
        self.push_body(&skin.body.model, skin.model, skin.tex_base, skin.scale, kinds::CAPTAIN, k, pos, rot, root, joints, alpha, flags);
        self.pianta = Some(skin);
    }

    /// The same for Louie.
    fn push_louie(&mut self, k: u64, pos: Vec3, rot: Quat, root: Mat4, joints: &[Mat4], anchors: &[Option<Mat4>; 15], alpha: f32, flags: u8) {
        let Some(skin) = self.louie.take() else { return };
        self.pending_anchors = Some(*anchors);
        self.push_body(&skin.body.model, skin.model, skin.tex_base, skin.scale, kinds::CAPTAIN, k, pos, rot, root, joints, alpha, flags);
        self.louie = Some(skin);
    }

    /// Skins a Sunshine-style body (`model`, baked into the view's textures from `tex_base` and listed as
    /// model `index`) from `joints` in its own units, `scale` and `root` over them, lowest vertex on the
    /// ground, and queues it as a captain draw.
    fn push_body(&mut self, m: &libsms_present::view::Model, index: u32, tex_base: i32, scale: f32, kind: u8, k: u64, pos: Vec3,
        rot: Quat, root: Mat4, joints: &[Mat4], alpha: f32, flags: u8) {
        let pose = libsms_assets::Pose::new(&m.bmd, root * Mat4::from_scale(Vec3::splat(scale)), joints);
        let low = m.batches.iter().flat_map(|b| &b.vertices).map(|v| pose.skin(v).0.y).fold(f32::INFINITY, f32::min);
        let low = if low.is_finite() && !std::mem::take(&mut self.no_lift) { low } else { 0.0 };
        if kind == kinds::CAPTAIN
            && let Some(raw) = self.pending_anchors.take()
        {
            let base = root * Mat4::from_scale(Vec3::splat(scale));
            let mut out = [None; 15];
            for (i, m) in raw.iter().enumerate() {
                out[i] = m.map(|mut m| { m = base * m; m.w_axis.y -= low; m });
            }
            self.captain_anchors.insert(k as u32, out);
        }
        let first_vertex = self.positions.len() as u32;
        let first_color = self.colors.len() as u32;
        for (bi, b) in m.batches.iter().enumerate() {
            let env = m.batch_env.get(bi).copied().flatten();
            for v in &b.vertices {
                let (mut p, n) = pose.skin(v);
                p.y -= low;
                let n = n.normalize_or_zero();
                self.positions.push(p);
                self.normals.push(n);
                self.uvs.push(match env {
                    Some(mtx) => {
                        let w = rot * n;
                        let [r, u, _] = self.camera;
                        libsms_present::tev::env_uv(w.dot(r), w.dot(u), mtx)
                    }
                    None => [v.uv[0].x, v.uv[0].y],
                });
            }
            let id = match m.batch_texture.get(bi).copied() {
                Some(t) if t >= 0 => t + tex_base,
                _ => -1,
            };
            let mut c = if id >= 0 { [1.0; 4] } else { m.batch_color_of(bi) };
            c[3] *= alpha;
            self.colors.push(c);
            self.texture_ids.push(id);
        }
        self.draws.push(Draw { key: k, kind, flags, model: index, pos, rot, first_vertex, first_color, alpha });
    }

    /// Loads Pikachu and the Ice Climbers from the Melee cache `dir` (with `PlPp.dat`/`PlNn.dat`, see
    /// `libssbm_assets::disc::extract_ice_climbers`), appending four models to the table: Pikachu, then
    /// the climbers for Blue, Red and Yellow Pikmin. Returns Pikachu's model index. Call before the host
    /// reads the table.
    pub fn load_pikachu(&mut self, dir: &std::path::Path) -> Result<u32, String> {
        if let Some(p) = &self.pikachu { return Ok(p.captain.index); }
        let olimar = &self.models[models::OLIMAR as usize].model;
        let height = crate::louie::olimar_height(olimar);
        let src_bind: Vec<Mat3> = libpikmin_assets::Skeleton::new(olimar).pose(Mat4::IDENTITY, None).joints.iter()
            .map(|j| Mat3::from_quat(j.to_scale_rotation_translation().1)).collect();
        let mut skin = crate::pikachu::PikachuSkin::load(dir, &src_bind, height)?;
        let first = self.models.len() as u32;
        let mut list = std::mem::take(&mut self.textures.list);
        let mut entries = Vec::new();
        for (i, body) in std::iter::once(&mut skin.captain).chain(skin.climbers.iter_mut()).enumerate() {
            body.register(first + i as u32, &mut list);
            let (lo, hi) = body.bounds();
            let (blend, lit) = body.blend_lit();
            let n = body.shown.len();
            entries.push(ViewModel::external(
                body.table_batches(), lo, hi,
                body.shown.iter().map(|&(_, t)| t).collect(), blend, vec![None; n], lit.iter().map(|l| l & 3).collect(),
            ));
        }
        self.textures.list = list;
        self.models.extend(entries);
        self.pikachu = Some(std::sync::Arc::new(skin));
        Ok(first)
    }

    /// Pikachu's height and a climber's, as multiples of Olimar's.
    pub fn set_pikachu_ratios(&mut self, captain: f32, climber: f32) {
        if let Some(p) = self.pikachu.as_mut().and_then(std::sync::Arc::get_mut) { p.set_ratios(captain, climber); }
    }

    /// Skins a Melee body from `joints` (its own units) with `root` over them, lowest vertex on the ground, and
    /// queues it as a draw of `kind`. `swarm`: a far one in a swarm may keep its last mesh (the pose budget).
    #[allow(clippy::too_many_arguments)]
    fn push_hsd(&mut self, body: &crate::pikachu::HsdBody, kind: u8, k: u64, pos: Vec3, rot: Quat, root: Mat4,
        joints: &[Mat4], alpha: f32, flags: u8, swarm: bool) {
        let retain_pose = swarm
            && self.posed_models.get(&k) == Some(&body.index)
            && self.pose_budget.is_some_and(|(eye, far)| pos.distance_squared(eye) > far * far)
            && (self.render_frame + (k & 0xffff_ffff)) % 3 != 0;
        let first_vertex = self.positions.len() as u32;
        let first_color = self.colors.len() as u32;
        if !retain_pose {
            let (mut p, mut n) = (Vec::new(), Vec::new());
            let low = body.skin_into(joints, root, &mut p, &mut n);
            let low = if low.is_finite() { low } else { 0.0 };
            if kind == kinds::CAPTAIN
                && let Some(raw) = self.pending_anchors.take()
            {
                let base = root * Mat4::from_scale(Vec3::splat(body.scale));
                let mut out = [None; 15];
                for (i, m) in raw.iter().enumerate() {
                    out[i] = m.map(|mut m| { m = base * m; m.w_axis.y -= low; m });
                }
                self.captain_anchors.insert(k as u32, out);
            }
            self.positions.extend(p.into_iter().map(|v| Vec3::new(v.x, v.y - low, v.z)));
            self.normals.extend(n);
            for &(src, _) in &body.shown {
                self.uvs.extend(body.model.batches[src].vertices.iter().map(|v| [v.uv.x, v.uv.y]));
            }
            self.posed_models.insert(k, body.index);
        }
        for (i, &(_, tex)) in body.shown.iter().enumerate() {
            let mut c = body.color(i);
            c[3] *= alpha;
            self.colors.push(c);
            self.texture_ids.push(tex);
        }
        let flags = flags | if retain_pose { flags::RETAIN_POSE } else { 0 };
        self.draws.push(Draw { key: k, kind, flags, model: body.index, pos, rot, first_vertex, first_color, alpha });
    }

    /// Loads the Louie body some captains can wear from the Pikmin 2 cache `dir` (`libsms::extract::p2`),
    /// appending it to the model table; returns its model index. Call before the host reads the table.
    pub fn load_louie(&mut self, dir: &std::path::Path) -> Result<u32, String> {
        if let Some(l) = &self.louie { return Ok(l.model); }
        let height = crate::louie::olimar_height(&self.models[models::OLIMAR as usize].model);
        let model = self.models.len() as u32;
        let tex_base = self.textures.list.len() as i32;
        let skin = crate::louie::LouieSkin::load(dir, model, tex_base, height)?;
        let m = &skin.body.model;
        let (lo, hi) = skin.bounds();
        self.textures.list.extend(skin.textures());
        self.models.push(ViewModel::external(
            skin.batches(), lo, hi,
            m.batch_texture.iter().map(|&t| if t >= 0 { t + tex_base } else { -1 }).collect(),
            m.batch_blend.clone(),
            m.batch_env.clone(),
            m.batch_lit.iter().map(|l| l & 3).collect(),
        ));
        self.louie = Some(skin);
        // His ship comes with him, when the cache has it.
        if let Err(e) = self.load_hocotate(dir) {
            eprintln!("Louie's ship: {e}");
        }
        // His navi light is optional: the same cache, the effects file.
        match crate::navilight::NaviLight::load(dir) {
            Ok(light) => self.navi_light = Some(light),
            Err(e) => eprintln!("Louie's navi light: {e}"),
        }
        Ok(model)
    }

    /// Loads Purple and White Pikmin from the Pikmin 2 cache `dir` (`libsms::extract::p2`), appending two models
    /// to the table (purple, then white); returns the purple one's index. Call before the host reads the table.
    /// Without it they are drawn as tinted Pikmin 1 bodies.
    pub fn load_pikmin2(&mut self, dir: &std::path::Path) -> Result<u32, String> {
        if let Some(p) = &self.pikmin2 { return Ok(p.purple.model); }
        let height = crate::louie::olimar_height(&self.models[models::PIKMIN[0] as usize].model);
        let first = self.models.len() as u32;
        let tex_base = self.textures.list.len() as i32;
        let skin = crate::pikmin2::Pikmin2Skin::load(dir, first, tex_base, height)?;
        let scale = libpikmin::Params::default().p2.new_colour_scale;
        for color in [libpikmin::PikiColor::Purple, libpikmin::PikiColor::White] {
            let b = skin.body(color).expect("both bodies");
            let m = &b.body.model;
            let (lo, hi) = skin.bounds(color, scale);
            self.textures.list.extend(skin.textures(color));
            self.models.push(ViewModel::external(
                skin.batches(color, scale), lo, hi,
                m.batch_texture.iter().map(|&t| if t >= 0 { t + b.tex_base } else { -1 }).collect(),
                m.batch_blend.clone(),
                m.batch_env.clone(),
                m.batch_lit.iter().map(|l| l & 3).collect(),
            ));
        }
        self.pikmin2 = Some(std::sync::Arc::new(skin));
        Ok(first)
    }

    /// Loads Louie's ship from the Pikmin 2 cache `dir`, appending its model to the table; returns the model index.
    /// Call before the host reads the table.
    pub fn load_hocotate(&mut self, dir: &std::path::Path) -> Result<u32, String> {
        if let Some(h) = &self.hocotate { return Ok(h.model); }
        let model = self.models.len() as u32;
        let tex_base = self.textures.list.len() as i32;
        let skin = crate::hocotate::HocotateSkin::load(dir, model, tex_base)?;
        let m = &skin.body.model;
        let (lo, hi) = skin.bounds();
        self.textures.list.extend(skin.textures());
        self.models.push(ViewModel::external(
            skin.batches(), lo, hi,
            m.batch_texture.iter().map(|&t| if t >= 0 { t + tex_base } else { -1 }).collect(),
            m.batch_blend.clone(),
            m.batch_env.clone(),
            m.batch_lit.iter().map(|l| l & 3).collect(),
        ));
        self.hocotate = Some(std::sync::Arc::new(skin));
        Ok(model)
    }

    /// The ship of Onion `onion` is Louie's Hocotate Ship (`on`) or the Dolphin again; ignored when it did not load.
    pub fn set_ship_hocotate(&mut self, onion: u32, on: bool) {
        if on { self.hocotate_ships.insert(onion); } else { self.hocotate_ships.remove(&onion); }
    }

    /// How tall Louie stands, as a multiple of Olimar.
    pub fn set_louie_height(&mut self, ratio: f32) {
        if let Some(l) = self.louie.as_mut() { l.set_height_ratio(ratio); }
    }

    /// First and last frame of the boarding clip worth playing: Olimar lying at the Onion's foot, then lifted
    /// into it (`DEMOID_BadEndingOlimin`; the clip is 30 frames a second).
    pub const BOARDING_FRAMES: (f32, f32) = (465.0, 552.0);

    /// Captain `captain` plays the boarding clip at `frame`, relative to the Onion whose foot is `at`; a negative
    /// frame ends it. False when the clip was not extracted.
    pub fn set_boarding(&mut self, captain: u32, frame: f32, at: Vec3) -> bool {
        if self.boarding.is_none() { return false; }
        if frame.is_finite() && frame >= 0.0 {
            self.boarding_clips.insert(captain, (frame, at));
        } else {
            self.boarding_clips.remove(&captain);
        }
        true
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

    /// Dolphin recovery particles at Olimar's current position, with one soul per stored
    /// Pikmin sacrificed. Progress outside 0..1 ends the attached healing sparkles.
    pub fn set_healing(&mut self, captain: u32, progress: f32, ghosts: u32) {
        if progress.is_finite() && (0.0..1.0).contains(&progress) {
            if self.healing.get(&captain).is_some_and(|&(previous, _)| progress < previous) {
                self.healing_active.remove(&captain);
            }
            self.healing.insert(captain, (progress, ghosts.min(100)));
        } else { self.healing.remove(&captain); }
    }

    /// Poses everything in `sim` for this frame. `alpha` is how far into the next tick the
    /// clock is ([`Sim::alpha`]), `dt` the frame time.
    pub fn update(&mut self, sim: &Sim, alpha: f32, dt: f32) {
        self.render_frame = self.render_frame.wrapping_add(1);
        self.draws.clear();
        self.light_spots.clear();
        self.treasure_spots.clear();
        self.treasure_bursts.retain(|b| self.time - b.3 < crate::treasure::BURST_SECONDS);
        self.bulb_spots.clear();
        self.bulb_bursts.retain(|b| self.time - b.3 < crate::bulb::BURST_SECONDS);
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
                    ShipFlight { animator, progress: None, flames: Vec::new(), ground_anchor: None,
                        trouble: false, trouble_spark_time: 0.0,
                        sucking: false,
                        seen: on_board.clone(), pending: VecDeque::new(), repair: None }
                });
                let mut cues = Vec::new();
                // `UfoItem::finishSuck`: a puff where a part goes in (spawned once the ship's facing is known).
                let mut gep = false;
                let sucking = sim.pellets().iter().any(|p| p.desc.ship_part.is_some()
                    && matches!(p.state, PelletState::Sucking { onion, .. } if onion == o.id));
                let was_sucking = flight.sucking;
                if sucking && !was_sucking && flight.repair.is_none() { flight.animator.suck_start(); }
                for &part in &on_board {
                    if !flight.seen.contains(&part) {
                        flight.seen.push(part);
                        flight.pending.push_back(part);
                    }
                }
                if o.flight_motion.is_none() {
                    if flight.repair.is_none() && let Some(part) = flight.pending.pop_front() {
                        if was_sucking || sucking {
                            let fitted = on_board.iter().position(|&p| p == part).map_or(1, |i| i + 1);
                            flight.animator.suck_finish(part as usize, params.ai.ship_level(fitted as u32), &mut cues);
                            gep = true;
                            flight.repair = Some((part, 0.5));
                        } else {
                            flight.animator.suck_start();
                            flight.repair = Some((part, 0.0));
                        }
                    }
                    if let Some((part, age)) = flight.repair {
                        let next = age + dt;
                        if age < 0.5 && next >= 0.5 {
                            let fitted = on_board.iter().position(|&p| p == part).map_or(1, |i| i + 1);
                            flight.animator.suck_finish(part as usize, params.ai.ship_level(fitted as u32), &mut cues);
                            gep = true;
                        }
                        flight.repair = if next > 6.0 || (next > 0.6 && flight.animator.repair_finished()) {
                            None
                        } else { Some((part, next)) };
                    }
                }
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
                    // Host extension: Ship Repair's first stage crash-lands the bare wreck. Pikmin 1's crash site
                    // (`kiso_t`'s effect keys 0 and 2, `UfoItem::animationKeyUpdated`) throws debris and a
                    // cloud of dust as the hull hits the ground.
                    if self.repair_mode && on_board.is_empty() {
                        for fx in fx_defs::ROCKET_NKE.into_iter().chain([fx_defs::CLOUD_OF_DUST_1]) {
                            cues.push(ShipCue::Effect { fx, lift: 0.0, normal: None });
                        }
                    }
                } else {
                    if o.flight_motion.is_some_and(|m| !m.0) && !flight.animator.taking_off() {
                        flight.pending.clear(); flight.repair = None;
                        flight.animator.take_off(&on_board, &mut cues);
                    }
                    flight.animator.update(dt, &mut cues);
                }
                flight.progress = landing;
                flight.sucking = sucking;
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
                // Host extension: the bare wreck (no part on board: Ship Repair's first stage) comes down already in
                // the pose it crashes in, nose down, not upright to be turned over on touching the ground.
                let crash = self.repair_mode && on_board.is_empty() && animator.landing();
                let mut pose = vm.skel.pose_by(Mat4::IDENTITY, |j| {
                    let ctx = self.ship_contexts.get(j).copied().unwrap_or(0);
                    let (motion, frame) = if ctx == 0 && crash {
                        animator.wreck_context()
                    } else if ctx == 0 && (!self.repair_mode || on_board.len() == 30) && !animator.landing() && !animator.taking_off() && flight.repair.is_none() && !sucking {
                        (11, 0.0)
                    } else { animator.context(ctx) };
                    let (_, anim) = self.ship_anims.get(motion)?;
                    Some((anim, frame.clamp(0.0, anim.frames.saturating_sub(1) as f32)))
                });
                let mut offset = ground_anchor - pose.joints[2].w_axis.truncate();
                if self.repair_mode && on_board.len() < 30 && (crash || !animator.landing() && !animator.taking_off()) {
                    // The original bare wreck lies on its side. Keep that original pose,
                    // and place its actual hull on the host's ground instead of forcing
                    // the fully repaired upright rocket pose onto every repair state.
                    let bottom = vm.batches.iter().flat_map(|b| &b.vertices)
                        .map(|v| pose.skin(v).0.y).fold(f32::INFINITY, f32::min);
                    offset.y = -bottom;
                }
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
                // The damaged ship smokes from its authored hull joints while it is
                // grounded. Keep the emitters attached as repairs change its pose.
                let wrecked = self.repair_mode && on_board.len() < 30
                    && o.flight_motion.is_none() && !animator.taking_off() && !animator.landing();
                let trouble_key = (7u64 << 48) | ((o.id.0 as u64) << 8);
                let trouble_points = [
                    ("joint2", Vec3::new(13.1, -98.4, -2.0)),
                    ("joint2", Vec3::new(-9.2, -68.1, 28.6)),
                    ("joint41", Vec3::new(-22.2, 4.9, -25.0)),
                    ("joint2", Vec3::new(0.0, -93.0, 0.0)),
                ];
                if let Some(e) = self.effects.as_mut() {
                    if wrecked && !flight.trouble {
                        for slot in [0usize, 1, 3] {
                            if let Some(j) = vm.model.joint_index(trouble_points[slot].0) {
                                let at = o.pos + rot * pose.joints[j].transform_point3(trouble_points[slot].1);
                                e.attach(trouble_key | slot as u64, fx_defs::ROCKET_TROUBLE[slot], at);
                            }
                        }
                    }
                    if wrecked {
                        for slot in [0usize, 1, 3] {
                            if let Some(j) = vm.model.joint_index(trouble_points[slot].0) {
                                e.move_attached(trouble_key | slot as u64,
                                    o.pos + rot * pose.joints[j].transform_point3(trouble_points[slot].1));
                            }
                        }
                        // `UfoItem::updateTroubleEffect`: an 80-frame loop; sparks fly (`rkt_hiba`, with
                        // `SE_UFO_SPARK`) at the first hull point on frame 0, the third on 13 and the second on 50.
                        let before = flight.trouble_spark_time;
                        let after = before + dt * 30.0;
                        for (key, slot) in [(0.0f32, 0usize), (13.0, 2), (50.0, 1)] {
                            let due = if key == 0.0 { before == 0.0 } else { before < key && after >= key };
                            if !due { continue; }
                            if let Some(j) = vm.model.joint_index(trouble_points[slot].0) {
                                e.spawn(fx_defs::ROCKET_TROUBLE[2],
                                    o.pos + rot * pose.joints[j].transform_point3(trouble_points[slot].1));
                                self.ship_sounds.push((o.id, "SE_UFO_SPARK", true));
                            }
                        }
                        flight.trouble_spark_time = if after >= 80.0 { 0.0 } else { after };
                    } else if flight.trouble {
                        flight.trouble_spark_time = 0.0;
                        e.detach_where(|k| k & !255 == trouble_key);
                    }
                }
                flight.trouble = wrecked;
                // `UfoItem::setSpotActive`: the rings and sparkles on the goal spot in front of the ship, which is
                // where it receives its parts, for as long as it stands landed.
                let spot_key = (10u64 << 48) | ((o.id.0 as u64) << 8);
                let goal = o.goal_pos();
                let lit = o.grounded() && o.flight_motion.is_none() && !animator.landing() && !animator.taking_off();
                if let Some(e) = self.effects.as_mut() {
                    if lit && self.ship_spots.insert(o.id.0) {
                        e.attach(spot_key, fx_defs::NAVI_RINGS, goal);
                        e.attach(spot_key | 1, fx_defs::NAVI_SPARKLE, goal);
                    } else if !lit && self.ship_spots.remove(&o.id.0) {
                        e.detach_where(|k| k & !255 == spot_key);
                    }
                    if lit {
                        e.move_attached(spot_key, goal);
                        e.move_attached(spot_key | 1, goal);
                    }
                }
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
                            {
                                // The landing reuses the takeoff clip's dust; at full alpha it
                                // shrouds Olimar, the squad and the Onion. Keep it, toned down.
                                if fx == fx_defs::ROCKET_LAND { g.alpha_scale = 0.3; }
                                if let Some(n) = normal { g.oriented_normal = rot * n; }
                            }
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
                if gep && let Some(e) = self.effects.as_mut()
                    && let Some(g) = e.spawn(fx_defs::ROCKET_GEP, o.suck_pos())
                {
                    g.emit_dir = rot * Vec3::Z;
                }
                if let Some(e) = self.effects.as_mut() {
                    for &(slot, id) in &flight.flames {
                        if let Some(at) = sphere(id) { e.move_attached(fx_key | slot as u64, at); }
                    }
                }
                // Louie's ship stands in for the Dolphin's body: sized to the upright Dolphin.
                let hocotate = self.hocotate.clone().filter(|_| self.hocotate_ships.contains(&o.id.0));
                let dolphin_h = if hocotate.is_some() {
                    Some(*self.dolphin_height.get_or_insert_with(|| {
                        let rest = vm.skel.pose_by(Mat4::IDENTITY, |j| {
                            let ctx = self.ship_contexts[j];
                            let (motion, frame) = if ctx == 0 { (11, 0.0) } else { animator.context(ctx) };
                            self.ship_anims.get(motion).map(|(_, a)| (a, frame))
                        });
                        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                        for v in vm.batches.iter().flat_map(|b| &b.vertices) {
                            let y = rest.skin(v).0.y;
                            lo = lo.min(y);
                            hi = hi.max(y);
                        }
                        if hi > lo { hi - lo } else { 100.0 }
                    }))
                } else { None };
                let at = self.place(key(kinds::SHIP, i as u64), o.pos, tick, alpha);
                if let (Some(h), Some(dh)) = (hocotate, dolphin_h) {
                    let joints = h.joints(sucking, self.time);
                    let scale = dh / h.height().max(1.0);
                    let root = Mat4::from_rotation_y(-h.goal_yaw);
                    self.push_body(&h.body.model, h.model, h.tex_base, scale, kinds::SHIP, key(kinds::SHIP, i as u64), at, rot, root, &joints, 1.0, 0);
                    continue;
                }
                let lit = |m: usize| Inputs { frame: Some(light_frames.get(m).copied().unwrap_or(0.0)), ..Inputs::default() };
                self.push(key(kinds::SHIP, i as u64), kinds::SHIP, models::SHIP, at, rot, pose, &lit, 1.0, 0);
                for (part, model, ppose) in part_poses {
                    self.push(key(kinds::SHIP_PART, ((o.id.0 as u64) << 8) | part as u64), kinds::SHIP_PART, model, at, rot, ppose, &no_inputs, 1.0, 0);
                }
                continue;
            }
            self.onion_light(sim, i, o, dt);
            let smoke_key = (8u64 << 48) | ((o.id.0 as u64) << 8);
            let flow_key = smoke_key | 1;
            // `GoalItem::playEffect`'s EFF_Rocket_TakeS: the lift's ground cloud. The disc runs it
            // from the takeoff clip's key; a host-flown Onion keeps it while it is flying near its
            // ground (descended), so a low hover, a boarding Onion and an Onion Ride all have it.
            let ground = self.onion_ground.get(&o.id.0).copied();
            let height = ground.map_or(0.0, |g| (o.pos.y - g).max(0.0));
            let cloud = !o.grounded()
                && o.anim != libpikmin::onion::OnionAnim::Away
                && ground.is_none_or(|_| height <= ONION_DESCENDED);
            if cloud {
                // A hair above the floor so the quads start out of the terrain.
                let at = Vec3::new(o.pos.x, ground.unwrap_or(o.pos.y) + 2.0, o.pos.z);
                if let Some(fx) = self.effects.as_mut() {
                    if self.onion_lift_smoke.insert(o.id.0) {
                        if let Some(g) = fx.attach(smoke_key, fx_defs::ROCKET_TAKE_S, at) {
                            // take_s.pcr is a one-pass burst. Run its authored 100-frame curve
                            // again and again while the Onion is aloft. (Do not rewrite
                            // `pcr.max_passes`: a copied `Arc` stops matching the effect table,
                            // which is how the particles are looked up for drawing.)
                            g.loop_forever = true;
                        }
                    }
                    fx.move_attached(smoke_key, at);
                    fx.attached_emission_scale(smoke_key,
                        (1.0 - height / ONION_CLOUD_FADE).clamp(0.0, 1.0));
                }
            } else if self.onion_lift_smoke.remove(&o.id.0) {
                if let Some(fx) = self.effects.as_mut() { fx.detach(smoke_key); }
            }
            let Some((motion, anim, before)) = self.animator.onion(&self.onion_anims, i, o, dt) else {
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
            if let Some(d) = o.dormancy {
                let colour = o.colors[0].index();
                if d.reviving {
                    // pikidemo.c FindOnyon's timed commands, extracted from main.dol;
                    // compressed to the host's three-second startup, alongside its clip.
                    let (commands, end) = &self.boot_sounds[colour];
                    let now = d.progress * end;
                    let prev = self.boot_progress.insert(o.id.0, now).unwrap_or(-0.001);
                    for &(at, cmd) in commands {
                        if at > prev && at <= now { self.demo_sounds.push(Some(([6, 4, 5][colour], cmd))); }
                    }
                }
                // AICreature::animationKeyUpdated -> GoalItem::playEffect. Use actual
                // event keys and the current collision spheres of the disc model.
                let now = anim.map_or(0.0, |(_, f)| f);
                let prev = before.unwrap_or(now);
                let keys: Vec<_> = self.onion_anims.get(motion).into_iter()
                    .flat_map(|(info, _)| &info.event_keys)
                    .filter(|e| e.event == 1 && (e.frame as f32) > now.min(prev) && (e.frame as f32) <= now.max(prev))
                    .map(|e| e.value).collect();
                let vm = &self.models[models::ONION as usize];
                let sphere = |id: &str| vm.model.coll.iter().find(|c| c.id == id)
                    .and_then(|c| pose.joints.get(c.joint).map(|m| o.pos + m.transform_point3(c.centre)))
                    .unwrap_or(o.pos);
                let flower = sphere("piki");
                let bases = [sphere("bas1"), sphere("bas2"), sphere("bas3")];
                if let Some(fx) = self.effects.as_mut() {
                    for value in keys {
                        match value {
                            0 => { for def in fx_defs::ONYON_BOOT_FLOW { fx.spawn(def, flower); } }
                            1 => { for def in if colour == 0 { &fx_defs::ONYON_BOOT_WATER[..] } else { &fx_defs::ONYON_BOOT_DUST[..] } { fx.spawn(*def, o.pos); } }
                            2 | 3 => {
                                for at in bases.into_iter().take(if value == 2 { 1 } else { 3 }) {
                                    fx.spawn(if colour == 0 { fx_defs::ONYON_BOOT_BUBBLES } else { fx_defs::SMOKE_RING_M }, at);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            } else if self.boot_progress.remove(&o.id.0).is_some()
                && !sim.onions().iter().any(|on| on.dormancy.is_some_and(|d| d.reviving)) {
                self.demo_sounds.push(None);
            }
            if o.dormancy.is_none() && (6..=12).contains(&motion) {
                // GoalItem::playEffect: disc keys drive flying sparkles, takeoff dust,
                // landing smoke, and the stocked Onion's idle sparkle/puff and wheeze.
                let now = anim.map_or(0.0, |(_, f)| f);
                let prev = before.unwrap_or(-1.0);
                let keys: Vec<_> = self.onion_anims.get(motion).into_iter()
                    .flat_map(|(info, _)| &info.event_keys)
                    .filter(|e| e.event == 1 && (if now >= prev {
                        (e.frame as f32) > prev && (e.frame as f32) <= now
                    } else {
                        (e.frame as f32) > prev || (e.frame as f32) <= now
                    }))
                    .map(|e| e.value).collect();
                let vm = &self.models[models::ONION as usize];
                let sphere = |id: &str| vm.model.coll.iter().find(|c| c.id == id)
                    .and_then(|c| pose.joints.get(c.joint).map(|m| o.pos + m.transform_point3(c.centre)))
                    .unwrap_or(o.pos);
                let bases = [sphere("bas1"), sphere("bas2"), sphere("bas3")];
                let puffs = [sphere("eff1"), sphere("eff2"), sphere("eff3")];
                let flower = sphere("piki");
                if let Some(fx) = self.effects.as_mut() {
                    // GoalItem::startLand enables setFlowEffect before the landing clip.
                    if (9..=11).contains(&motion) && before.is_none() {
                        fx.attach(flow_key, fx_defs::KAFUN_NG, flower);
                    }
                    for value in keys {
                        match (motion, value) {
                            // GoalItem::setFlowEffect: EFF_Kafun_NG (296), attached
                            // to the flower's 'piki' collision sphere, not ground smoke.
                            (6..=8, 0) => { fx.attach(flow_key, fx_defs::KAFUN_NG, flower); }
                            (6..=8, 2) => { /* Attached ground smoke above handles the takeoff key. */ }
                            (9..=11, 0) => { fx.spawn(fx_defs::ROCKET_LAND_S, o.pos); }
                            (9..=11, 1) => { fx.spawn(fx_defs::ROCKET_SMOKE_D, bases[0]); }
                            (9..=11, 2) => { for at in bases { fx.spawn(fx_defs::ROCKET_SMOKE_D, at); } }
                            (9..=11, 3) => { fx.detach(flow_key); }
                            (12, value) => {
                                if value == 0 { fx.spawn(fx_defs::ONYON_SPARKLES, flower); }
                                fx.spawn(fx_defs::ONYON_PUFF, puffs[((o.id.0 as usize) + value as usize) % 3]);
                                self.ship_sounds.push((o.id, "SE_CONTAINER_PIKI", true));
                            }
                            _ => {}
                        }
                    }
                    fx.move_attached(flow_key, flower);
                    if motion == 12 { fx.detach(flow_key); }
                }
            } else if let Some(fx) = self.effects.as_mut() {
                fx.detach(flow_key);
            }
            // GoalItem::refresh: even frames live, odd frames dormant, per colour.
            let frame = o.colors[0].index() as f32 * 2.0
                + o.dormancy.map_or(0.0, |d| ((1.0 - d.progress) * 4.0).round() * 0.25);
            let inputs = move |_: usize| Inputs { frame: Some(frame), ..Default::default() };
            let pos = self.place(k, o.pos, tick, alpha);
            // A flying Onion tips toward its travel, like a helicopter (the disc clips only ever
            // rise or descend, so the host's own trajectory supplies the direction).
            let lean = {
                let state = self.onion_lean.entry(o.id.0).or_insert((tick, o.pos, OnionLean::default()));
                if state.0 != tick {
                    let seconds = tick.saturating_sub(state.0).max(1) as f32 / 30.0;
                    state.2.advance(state.1, o.pos, seconds, !o.grounded());
                    state.0 = tick;
                    state.1 = o.pos;
                }
                state.2.quat
            };
            self.push(k, kinds::ONION, models::ONION, pos, lean, pose, &inputs, 1.0, 0);
        }
        self.arrivals.clear();

        // FallWaterAI / WaterAI, using the same animation selection as the
        // original viewer's nectar path. Keep the authored root height.
        for d in sim.nectar_drops() {
            use libpikmin::nectar::DropState;
            let (motion,frame)=match d.state {
                DropState::Falling{frame,leaving:true}=>(1,frame),
                DropState::Falling{frame,..}|DropState::Splash{frame,..}=>(0,frame),
            };
            let model=self.nectar_models[0];
            let anim=self.nectar_anims[0].get(motion).map(|(_,a)|(a,frame.min(a.frames.saturating_sub(1) as f32)));
            let pose=self.models[model as usize].skel.pose(Mat4::IDENTITY,anim);
            self.no_lift=true;
            self.push(key(kinds::NECTAR_DROP,d.id.0 as u64),kinds::NECTAR_DROP,model,d.pos,Quat::IDENTITY,pose,&no_inputs,1.0,0);
        }
        for n in sim.puddles() {
            use libpikmin::PuddleState::*;
            let (motion,frame)=match n.state {Wait{frame}=>(0,frame),Touched{frame}=>(1,frame),Appear{frame}=>(2,frame),Drain{frame}=>(3,frame)};
            let model=self.nectar_models[1];
            let anim=self.nectar_anims[1].get(motion).map(|(_,a)|(a,frame.min(a.frames.saturating_sub(1) as f32)));
            let pose=self.models[model as usize].skel.pose(Mat4::IDENTITY,anim);
            self.no_lift=true;
            self.push(key(kinds::NECTAR_PUDDLE,n.id.0 as u64),kinds::NECTAR_PUDDLE,model,n.pos,Quat::IDENTITY,pose,&no_inputs,1.0,0);
        }

        // Pellets and corpses: the numbered pellets, sized to the thing being carried
        // (corpses too: a host's enemies have no Pikmin model). The model goes by carriers,
        // which for a numbered pellet is its number.
        if let Some(set) = self.bulbs.clone() {
            for bulb in sim.bulbs() {
                let Some(body) = set.body(bulb.colour) else { continue };
                // Its moments are followed wherever it is (so a bud met again is not "new"), but only buds within
                // `DRAW_DISTANCE` of a captain are posed, skinned, heard or lit: the rest are out of sight anyway.
                let (cues, burst, track) = crate::bulb::moments(self.bulb_tracks.get(&bulb.id.0).copied(), bulb);
                self.bulb_tracks.insert(bulb.id.0, track);
                if !sim.captains().is_empty() && !sim.captains().iter().any(|c| {
                    let d = c.pos - bulb.pos;
                    d.x * d.x + d.z * d.z < crate::bulb::DRAW_DISTANCE * crate::bulb::DRAW_DISTANCE
                }) {
                    continue;
                }
                let joints = set.joints(bulb);
                let k = key(kinds::PELLET, 0x4255_0000 + bulb.id.0 as u64);
                let (pos, rot) = self.place_rot(k, bulb.pos, Quat::IDENTITY, tick, alpha);
                // The Pop is a little taller than the plain buds; no-lift keeps the foot where the sim has it.
                let scale = if bulb.queen { 1.25 } else { 1.0 };
                self.push_body(&body.body.model, body.model, body.tex_base, scale, kinds::PELLET, k, pos, rot,
                    Mat4::IDENTITY, &joints, 1.0, 0);
                // Its sounds (on the host's Pikmin 2 output, with the treasures') and the effects of the moment.
                let now = self.time;
                self.treasure_cues.extend(cues.into_iter().map(|id| crate::treasure::Cue { id, at: bulb.pos }));
                if let Some(kind) = burst { self.bulb_bursts.push((bulb.pos, bulb.colour, kind, now)); }
                if bulb.can_swallow() { self.bulb_spots.push((bulb.pos, bulb.colour, bulb.id.0)); }
            }
            if self.bulb_tracks.len() > sim.bulbs().len() {
                self.bulb_tracks.retain(|id, _| sim.bulbs().iter().any(|b| b.id.0 == *id));
            }
        }
        for pel in sim.pellets() {
            let treasure = pel.desc.ship_part.filter(|&p| p >= libpikmin::TREASURE_PART_BASE && self.treasures.is_some());
            if let Some(part) = treasure {
                // A treasure: its moments (sounds, bursts) first, so the one that is gone still gets its finish.
                let state = match pel.state {
                    PelletState::Carried { .. } => 1,
                    PelletState::Sucking { .. } => 2,
                    PelletState::Gone => 3,
                    _ => 0,
                };
                let now = self.time;
                let (cues, burst, track) = crate::treasure::moments(self.treasure_tracks.get(&pel.id.0).copied(), state, pel.pos, pel.desc.radius, now);
                self.treasure_cues.extend(cues);
                if let Some(kind) = burst { self.treasure_bursts.push((pel.pos, pel.desc.radius, kind, now)); }
                match track {
                    Some(t) => { self.treasure_tracks.insert(pel.id.0, t); }
                    None => { self.treasure_tracks.remove(&pel.id.0); }
                }
                if let (Some(t), Some(set)) = (track, self.treasures.clone())
                    && let Some(body) = set.body((part - libpikmin::TREASURE_PART_BASE) as usize)
                {
                    let joints = set.joints((part - libpikmin::TREASURE_PART_BASE) as usize, state != 0, now - t.carry_began);
                    let k = key(kinds::PELLET, pel.id.0 as u64);
                    let (pos, rot) = self.place_rot(k, pel.pos, Quat::from_rotation_y(pel.yaw), tick, alpha);
                    self.push_body(&body.treasure.model, body.model, body.tex_base, pel.scale, kinds::PELLET, k, pos, rot,
                        Mat4::IDENTITY, &joints, 1.0, 0);
                    if state != 2 { self.treasure_spots.push((pel.pos, pel.desc.radius, pel.id.0)); }
                }
                continue;
            }
            if pel.state == PelletState::Gone {
                continue;
            }
            if let Some(part) = pel.desc.ship_part
                && let Some(Some(pv)) = self.parts.get(part as usize)
            {
                let model = pv.model;
                let cfg = params.pellets.iter().find(|c| c.kind == 3 &&
                    (c.model_id == libpikmin_assets::ship::PART_IDS[part as usize] || c.pellet_id == libpikmin_assets::ship::PART_IDS[part as usize]));
                let scale = cfg.map_or(1.0, |c| c.scale) * pel.scale;
                let anim = pv.anims.as_ref().and_then(|a| a.get(0)).map(|(info, a)| {
                    let end = info.keys.last().copied().unwrap_or(1).max(1) as f32;
                    (a, (self.time * 30.0) % end)
                });
                let vm = &self.models[model as usize];
                let pose = vm.skel.pose(Mat4::from_scale(Vec3::splat(scale)), anim);
                let at = pel.pos + Vec3::Y * (-vm.lo.y * scale);
                let k = key(kinds::PELLET, pel.id.0 as u64);
                let (pos, rot) = self.place_rot(k, at, Quat::from_rotation_y(pel.yaw), tick, alpha);
                self.push(k, kinds::PELLET, model, pos, rot, pose, &no_inputs, 1.0, 0);
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

        // The host's enemies, each in the pose it names (`PaniTekiAnimator` plays the `.key`
        // motions directly).
        self.teki_positions.clear();
        let mut heard = HashSet::new();
        for t in std::mem::take(&mut self.teki_input) {
            // Pikmin 2's enemies: a J3D body on a `.bca` clip, at the size the enemy reports.
            if let Some(p2) = self.p2_tekis.iter().find(|p| p.info.kind == t.kind).cloned() {
                let scale = p2.info.scale * t.scale.max(0.0);
                if scale < 0.01 { continue; }
                let cid = teki_creature_id(t.id);
                heard.insert(t.id);
                self.teki_positions.push((cid, t.pos));
                let bind = p2.body.bind_world();
                let mut joints = p2.clip(t.motion)
                    .and_then(|c| p2.body.clip_frames(c).and_then(|n| p2.body.pose(c, t.frame.clamp(0.0, (n - 1.0).max(0.0)))))
                    .unwrap_or(bind);
                // A Long Legs boss stands on its own feet: its legs fold onto them, it is drawn where its legs put it, and its height
                // is theirs (not the lowest vertex's).
                let (mut at, mut yaw) = (t.pos, t.yaw);
                if p2.legs.is_some() {
                    self.no_lift = true;
                    if let Some(lv) = self.teki_legs.get(&t.id).filter(|lv| lv.active) {
                        p2.fold_legs(&mut joints, lv);
                        (at, yaw) = (lv.trace, lv.face);
                    }
                }
                if let Some(fx)=self.p2_effects.as_mut() {fx.enemy(&t,&p2,&joints,at,yaw,scale);}
                let flags = if t.flash > 0.0 { flags::FLASH } else { 0 };
                let k = key(kinds::TEKI, t.id as u64);
                self.push_body(&p2.body.model, p2.model, p2.tex_base, scale, kinds::TEKI, k, at, Quat::from_rotation_y(yaw),
                    Mat4::IDENTITY, &joints, t.alpha.clamp(0.0, 1.0), flags);
                continue;
            }
            let Some(view) = self.tekis.iter().find(|v| v.info.kind == t.kind) else { continue };
            // Sound keys passed since the last update: within the motion, or from its start when it changed
            // or looped round.
            let cid = teki_creature_id(t.id);
            heard.insert(t.id);
            self.teki_positions.push((cid, t.pos));
            if let Some(keys) = view.sound_keys.get(t.motion as usize) {
                let (from, to) = match self.teki_played.get(&t.id) {
                    Some(&(m, f)) if m == t.motion && f <= t.frame => (f, t.frame),
                    Some(&(m, f)) if m == t.motion => {
                        // Looped: the rest of the old pass, then the start of the new.
                        for &(_, i) in keys.iter().filter(|k| k.0 >= f) {
                            self.teki_sounds.push((cid, view.base, i));
                        }
                        (0.0, t.frame)
                    }
                    Some(_) => (0.0, t.frame),
                    None => (t.frame, t.frame),
                };
                for &(at, i) in keys {
                    if at >= from && at < to { self.teki_sounds.push((cid, view.base, i)); }
                }
            }
            self.teki_played.insert(t.id, (t.motion, t.frame));
            let model = view.info.model;
            let anim = view.anims.get(t.motion as usize)
                .map(|(_, a)| (a, t.frame.clamp(0.0, a.frames.saturating_sub(1) as f32)));
            let scale = view.info.scale * t.scale.max(0.0);
            let pose = self.models[model as usize].skel.pose(srt_matrix(Vec3::splat(scale), Vec3::ZERO, Vec3::ZERO), anim);
            let flags = if t.flash > 0.0 { flags::FLASH } else { 0 };
            let k = key(kinds::TEKI, t.id as u64);
            // BTeki::refresh: AnimationType 1 animates materials at _3BC,
            // which TaiPalmChangingColorAction advances independently of its motion.
            let inputs = |_| Inputs { frame: t.material_frame, ..Default::default() };
            self.push(k, kinds::TEKI, model, t.pos, Quat::from_rotation_y(t.yaw), pose, &inputs, t.alpha.clamp(0.0, 1.0), flags);
        }

        self.teki_played.retain(|id, _| heard.contains(id));

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
            live.insert(crate::animator::piki_key(p));
            let k = key(kinds::PIKI, crate::animator::piki_key(p));
            // A Pikmin on a flying Onion's leg is placed by the sim in the Onion's un-leaned
            // frame; draw it on the leg the Onion is actually drawn with, so the host's lean
            // does not slide the climbers off.
            let (world, rot) = match p.state {
                PikiState::Enter { onion, .. } | PikiState::Exit { onion, .. } => {
                    if let Some(o) = sim.onions().iter().find(|o| o.id == onion) {
                        let lean = self.onion_lean.get(&onion.0).map(|s| s.2.quat).unwrap_or(Quat::IDENTITY);
                        (o.pos + lean * (p.pos - o.pos), lean * Quat::from_rotation_y(p.face))
                    } else {
                        (p.pos, Quat::from_rotation_y(p.face))
                    }
                }
                _ => (p.pos, Quat::from_rotation_y(p.face)),
            };
            let (pos, rot) = self.place_rot(k, world, rot, tick, alpha);
            // Frozen: the pose held where it was, the body paled toward ice.
            let frozen = p.frozen > 0.0;
            let anim = if self.celebrating {
                libpikmin_assets::motion::Motion::RotJump.anim_index().and_then(|i| self.piki_anims.get(i))
                    .map(|(info, a)| (a, (self.time * 30.0 + p.id.0 as f32 * 7.0) % info.keys.last().copied().unwrap_or(1).max(1) as f32))
            } else { self.animator.piki(&self.piki_anims, p, &look, if frozen { 0.0 } else { dt }) };
            // Purple and White: Pikmin 2's bodies on their own motions, the Pikmin 1 clip's progress choosing the frame.
            let phase = anim.map(|(_, f)| {
                let len = look.motion.anim_index().and_then(|i| self.piki_anims.get(i))
                    .and_then(|(info, _)| info.keys.last().copied()).unwrap_or(1).max(1) as f32;
                (f / len).clamp(0.0, 1.0)
            }).unwrap_or(0.0);
            let skin2 = self.pikmin2.clone().filter(|_| p.color.is_pikmin2());
            if let Some(sk) = skin2 {
                let joints = sk.joints(p.color, look.motion, phase);
                let b = sk.body(p.color).expect("a Pikmin 2 colour");
                let s = sk.scale(p.color, params.p2.new_colour_scale);
                let (index, tex_base) = (b.model, b.tex_base);
                let root = Mat4::from_scale(look.scale);
                self.push_body(&b.body.model, index, tex_base, s, kinds::PIKI, k, pos, rot, root, &joints, look.fade, 0);
                if let Some(j) = joints.get(crate::pikmin2::HAPPA_JOINT).copied()
                    && !matches!(p.state, PikiState::Pressed { .. })
                {
                    let model = models::HAPPA[p.happa as usize];
                    let pose = self.models[model as usize].skel.pose(root * Mat4::from_scale(Vec3::splat(s)) * j, None);
                    let k = key(kinds::HAPPA, crate::animator::piki_key(p));
                    self.push(k, kinds::HAPPA, model, pos, rot, pose, &no_inputs, look.fade, 0);
                }
                continue;
            }
            let model = models::PIKMIN[p.color.index().min(2)];
            let pose = self.models[model as usize].skel.pose(srt_matrix(look.scale, Vec3::ZERO, Vec3::ZERO), anim);
            let head = pose.joints.get(6).copied();
            let tint = if frozen { icy(piki_rgb(p.color)) } else { piki_rgb(p.color) };
            let inputs = move |m: usize| Inputs { material: (m == 0).then_some(tint), ..Default::default() };
            let f = if frozen { flags::FROZEN } else { 0 };
            // A Pikachu captain's squad is Ice Climbers, on the Pikmin's own motion.
            let climber = self.pikachu.clone().filter(|_| {
                p.captain.is_some_and(|c| self.captain_skins.get(&c.0) == Some(&crate::pikachu::SKIN_PIKACHU))
            });
            if let Some(sk) = climber {
                let body = &sk.climbers[sk.costume(p.color, p.id.0)];
                let world = body.pose(&pose.joints);
                self.push_hsd(body, kinds::PIKI, k, pos, rot, Mat4::from_scale(look.scale), &world, look.fade, f & !flags::FROZEN, true);
                // Climbers have no Pikmin leaf, bud or flower attachment.
                continue;
            }
            self.push(k, kinds::PIKI, model, pos, rot, pose, &inputs, look.fade, f);
            if let Some(j) = head
                && !matches!(p.state, PikiState::Pressed { .. })
            {
                let model = models::HAPPA[p.happa as usize];
                let pose = self.models[model as usize].skel.pose(j, None);
                let k = key(kinds::HAPPA, crate::animator::piki_key(p));
                self.push(k, kinds::HAPPA, model, pos, rot, pose, &no_inputs, look.fade, 0);
            }
        }

        // Captains, and each one's cursor with the marker under it (`Navi::refresh`).
        for c in sim.captains() {
            live.insert(crate::animator::captain_key(c.id));
            let k = key(kinds::CAPTAIN, c.id.0 as u64);
            let (mut pos, rot) = self.place_rot(k, c.pos, Quat::from_rotation_y(c.face), tick, alpha);
            let mut anim = if self.celebrating {
                libpikmin_assets::motion::Motion::WaveJmp.anim_index().and_then(|i| self.piki_anims.get(i))
                    .map(|(info, a)| (a, (self.time * 30.0) % info.keys.last().copied().unwrap_or(1).max(1) as f32))
            } else { self.animator.captain(&self.piki_anims, &params, c, dt) };
            let mut root = crate::animator::captain_root(&params, c);
            // Boarding an Onion: the bad ending's motion, in the Onion's own space.
            let clip = self.boarding_clips.get(&c.id.0).copied().zip(self.boarding.as_ref());
            if let Some(((frame, at), body)) = clip {
                pos = at;
                anim = Some((body, frame));
                root = Mat4::IDENTITY;
            }
            let pianta_body = self.pianta.is_some() && self.captain_skins.get(&c.id.0) == Some(&crate::pianta::SKIN_PIANTA);
            let louie_body = self.louie.is_some() && self.captain_skins.get(&c.id.0) == Some(&crate::louie::SKIN_LOUIE);
            let pikachu_body = self.pikachu.is_some() && self.captain_skins.get(&c.id.0) == Some(&crate::pikachu::SKIN_PIKACHU);
            if pianta_body || louie_body || pikachu_body {
                // The Pianta jumps in Mario's clips, not tipped over on Olimar's geyser ride.
                root = Mat4::IDENTITY;
            }
            let fx_key = (8u64 << 48) | ((c.id.0 as u64) << 8);
            let heal_key = (9u64 << 48) | ((c.id.0 as u64) << 8);
            if let Some(&(_, ghosts)) = self.healing.get(&c.id.0) {
                if let Some(fx) = self.effects.as_mut() {
                    if self.healing_active.insert(c.id.0) {
                        fx.spawn(fx_defs::NAVI_RECOVER, pos);
                        fx.attach(heal_key, fx_defs::NAVI_SPARKLE, pos);
                        fx.attach(heal_key | 1, fx_defs::NAVI_RINGS, pos);
                        for i in 0..ghosts {
                            let angle = i as f32 * 2.399_963;
                            let radius = 8.0 + (i % 3) as f32 * 4.0;
                            let at = pos + Vec3::new(angle.cos() * radius, 4.0 + (i % 4) as f32 * 3.0, angle.sin() * radius);
                            fx.spawn(fx_defs::PIKI_DEAD_SOUL, at);
                        }
                    }
                    fx.move_attached(heal_key, pos);
                    fx.move_attached(heal_key | 1, pos);
                }
            } else if self.healing_active.remove(&c.id.0) && let Some(fx) = self.effects.as_mut() {
                fx.detach_where(|k| k & !255 == heal_key);
            }
            if let Some(&p) = self.materialize.get(&c.id.0) {
                // Host extension: transport from the Dolphin's hatch to the captain's
                // held arrival pose, using NaviUfoState's own recovery effects.
                if let Some(ship) = sim.onions().iter().filter(|o| o.ship.is_some())
                    .min_by(|a, b| a.pos.distance(c.pos).total_cmp(&b.pos.distance(c.pos))) {
                    pos = (ship.suck_pos() + Vec3::Y * 30.0).lerp(pos, p.clamp(0.0, 1.0));
                }
                // Beamed in: a thin tall figure that fills out, in the ship's healing effects
                // (`NaviUfoState`'s recover burst, the goal spot's sparkles).
                if let Some(fx) = self.effects.as_mut() {
                    if self.materializing.insert(c.id.0) {
                        fx.spawn(fx_defs::NAVI_RECOVER, pos);
                        fx.attach(fx_key, fx_defs::NAVI_SPARKLE, pos);
                        // The Dolphin's own doorstep rings (`UfoItem::setSpotActive`), on its goal
                        // spot nearest him, and its light beam from the hatch down to that spot
                        // (`UfoItem::startConeEffect`).
                        let ship = sim
                            .onions()
                            .iter()
                            .filter(|o| o.ship.is_some())
                            .min_by(|a, b| a.goal_pos().distance(c.pos).total_cmp(&b.goal_pos().distance(c.pos)));
                        if let Some(o) = ship {
                            fx.attach(fx_key | 1, fx_defs::NAVI_RINGS, o.goal_pos());
                            fx.attach_beam(fx_key | 2, o.suck_pos(), o.goal_pos());
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
            let (motion, _) = crate::animator::captain_motion(&params, c, self.animator.throwing(c.id));
            let phase = motion.anim_index().and_then(|i| self.piki_anims.get(i)).zip(anim).map_or(0.0, |((info, _), (_, frame))| {
                let (a, b) = (*info.keys.first().unwrap_or(&0) as f32, *info.keys.last().unwrap_or(&1) as f32);
                if b > a { ((frame - a) / (b - a)).rem_euclid(1.0) } else { 0.0 }
            });
            let flash = c.damage_frames > 0.0 && (c.damage_frames as i32 / 2) % 2 == 0;
            let f = if flash { flags::FLASH } else { 0 };
            // The whistle's beam starts at the antenna (`Navi::demoDraw`'s 'ante' sphere).
            let mut antenna = self
                .ante
                .as_ref()
                .and_then(|a| Some(pos + rot * pose.joints.get(a.joint)?.transform_point3(a.centre)))
                .unwrap_or(pos + Vec3::Y * 30.0);
            // The Pianta wears his palm where Olimar has his antenna.
            let mut pianta_joints = None;
            if pianta_body {
                let time = self.time;
                let rest = self.models[models::OLIMAR as usize].skel.pose(Mat4::IDENTITY, anim);
                if let Some(skin) = self.pianta.as_mut() {
                    let joints = skin.joints(c.id.0, c, motion, phase, time, params.navi.run_speed * c.speed_scale, &rest);
                    let head = skin.body.rig.find("yashi_jnt").and_then(|j| joints.get(j));
                    if let Some(h) = head {
                        antenna = pos + rot * (h.w_axis.truncate() * skin.scale);
                    }
                    pianta_joints = Some(joints);
                }
            }
            self.whistles.push(WhistleFrame {
                captain: c.id,
                state: c.whistle,
                antenna,
                cursor: c.cursor_world,
                captain_pos: pos,
                radius: c.whistle_radius(&params),
            });
            let pikachu_world = if pikachu_body {
                let rest = self.models[models::OLIMAR as usize].skel.pose(Mat4::IDENTITY, anim);
                let time = self.time;
                self.pikachu.as_ref().map(|p| p.captain_world(motion, phase, time, &rest.joints))
            } else { None };
            let louie_joints = if louie_body { self.louie.as_ref().map(|l| l.joints(c, motion, phase)) } else { None };
            // The host's attachment points on whichever body this captain wears. The Pianta and
            // Louie rigs name their joints, so the mapping survives their different joint counts;
            // Olimar's 12-joint rig and Pikachu's retarget are indexed (pikachu.rs::PARTS).
            let mut anchors: [Option<Mat4>; 15] = [None; 15];
            if let Some(joints) = pianta_joints.as_ref() {
                if let Some(skin) = self.pianta.as_ref() {
                    let at = |name: &str| named(&skin.body.rig, joints, name);
                    anchors[0] = at("kubi");
                    anchors[1] = at("koshi_null");
                    anchors[2] = at("body_jnt");
                    anchors[3] = at("armLT_jnt");
                    anchors[4] = at("armRT_jnt");
                    anchors[5] = at("armLB_jnt");
                    anchors[6] = at("armRB_jnt");
                    anchors[7] = at("handL_jnt");
                    anchors[8] = at("handR_jnt");
                    anchors[9] = at("legLT_jnt");
                    anchors[10] = at("legRT_jnt");
                    anchors[11] = at("legLB_jnt");
                    anchors[12] = at("legRB_jnt");
                    anchors[13] = at("footL_jnt");
                    anchors[14] = at("footR_jnt");
                }
            } else if let Some(joints) = louie_joints.as_ref() {
                if let Some(skin) = self.louie.as_ref() {
                    let at = |name: &str| named(&skin.body.rig, joints, name);
                    anchors[0] = at("kubi");
                    anchors[1] = at("koshi_null");
                    anchors[2] = at("body_jnt");
                    anchors[3] = at("armLT_jnt");
                    anchors[4] = at("armRT_jnt");
                    anchors[5] = at("armLB_jnt");
                    anchors[6] = at("armRB_jnt");
                    anchors[7] = at("handL_jnt");
                    anchors[8] = at("handR_jnt");
                    anchors[9] = at("legLT_jnt");
                    anchors[10] = at("legRT_jnt");
                    anchors[11] = at("legLB_jnt");
                    anchors[12] = at("legRB_jnt");
                    anchors[13] = at("footL_jnt");
                    anchors[14] = at("footR_jnt");
                }
            } else if let Some(world) = pikachu_world.as_ref() {
                if let Some(skin) = self.pikachu.as_ref() {
                    let at = |olimar: usize| skin.captain.pair_joint(olimar).and_then(|j| world.get(j)).copied();
                    anchors[0] = at(2);
                    anchors[1] = at(1);
                    anchors[2] = at(0);
                    anchors[7] = at(8);
                    anchors[8] = at(7);
                    anchors[9] = at(11);
                    anchors[10] = at(10);
                }
            } else {
                let at = |j: usize| pose.joints.get(j).copied();
                anchors[0] = at(2);
                anchors[1] = at(1);
                anchors[2] = at(0);
                anchors[7] = at(8);
                anchors[8] = at(7);
                anchors[9] = at(11);
                anchors[10] = at(10);
            }
            self.captain_anchors.insert(c.id.0, anchors);
            self.captain_anchors.insert(c.id.0, anchors);
            if let Some(joints) = pianta_joints {
                self.push_pianta(k, pos, rot, root, &joints, &anchors, 1.0, f);
            } else if let Some(joints) = louie_joints {
                self.push_louie(k, pos, rot, root, &joints, &anchors, 1.0, f);
                if let Some(l) = &self.louie {
                    self.light_spots.push((pos, l.scale * l.body.height()));
                }
            } else if let Some(world) = pikachu_world {
                if let Some(sk) = self.pikachu.clone() {
                    self.pending_anchors = Some(anchors);
                    self.push_hsd(&sk.captain, kinds::CAPTAIN, k, pos, rot, root, &world, 1.0, f, false);
                }
            } else {
                self.captain_anchors.insert(c.id.0, anchors);
                self.push(k, kinds::CAPTAIN, models::OLIMAR, pos, rot, pose, &no_inputs, 1.0, f);
            }
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

        if let Some(fx)=self.p2_effects.as_mut() {fx.advance((self.time as f64*30.0).floor() as u64);}
        self.animator.retain_players(&live);
        let drawn: HashSet<u64> = self.draws.iter().map(|d| d.key).collect();
        self.interp.retain(|k, _| drawn.contains(k));
        self.posed_models.retain(|k, _| drawn.contains(k));
    }
}
