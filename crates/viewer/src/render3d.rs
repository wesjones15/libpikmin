//! 3D view: draws the sim with Olimar, Pikmin, leaves, Onions and pellets loaded from the
//! user's extracted disc assets (`libpikmin-assets`), posed by the disc's own animations.
//!
//! Skinning and lighting run on the CPU (a few hundred vertices per Pikmin); materials go
//! through a small TEV interpreter ([`crate::tev`]) so they combine textures and colour
//! registers the way the game sets them up. Which animation plays is picked from sim state
//! the way the game's states do (`Piki::updateWalkAnimation`, `Navi::updateWalkAnimation`,
//! the `startMotion` calls in pikiState.cpp, naviState.cpp, aiAttack.cpp and aiTransport.cpp).
//! Where the sim keeps its own animation counter (swing, latch, lift, pluck, punch, death)
//! that frame is shown, so what you see matches the gameplay timing exactly.

use std::collections::HashMap;

use glam::{Mat4, Vec3 as GVec3};
use libpikmin::captain::{ThrowState, WhistleState};
use libpikmin::onion::{OnionAnim, OnionId, PluckPhase};
use libpikmin::piki::AttackMode;
use libpikmin::{
    CaptainId, Event, Params, PelletState, PikiColor, PikiState, Sim, SproutState, Vec2, Vec3,
};
use libpikmin_assets::model::{TexAttr, mat_flags};
use libpikmin_assets::particle::effects;
use libpikmin_assets::motion::Motion;
use libpikmin_assets::pose::srt_matrix;
use libpikmin_assets::ship::ShipAnimator;
use libpikmin_assets::{Anim, AnimSet, Assets, Batch, Blend, Model, Skeleton, paths};
use macroquad::miniquad::TextureWrap;
use macroquad::prelude::{
    Camera, Camera3D, Color, FilterMode, Mesh, Texture2D, Vertex, draw_line_3d, draw_mesh,
    screen_height, screen_width, set_camera, set_default_camera, vec2, vec3, vec4,
};

use crate::effects::{Effects, WhistleFrame};
use crate::teki::{Ai, TekiKind, Tekis};
use crate::tev::{self, DrawParams, Program, Tev};

fn mq(v: GVec3) -> macroquad::math::Vec3 {
    vec3(v.x, v.y, v.z)
}

/// A material ready to draw: its TEV program and up to four texture maps.
struct GpuMaterial {
    program: Program,
    textures: [Option<Texture2D>; 4],
    blend: Blend,
    lit: bool,
    specular: bool,
    vertex_color: bool,
}

struct GpuModel {
    model: Model,
    skel: Skeleton,
    batches: Vec<Batch>,
    materials: Vec<GpuMaterial>,
    /// Bind-pose bounds.
    lo: GVec3,
    hi: GVec3,
}

fn wrap(bits: u16) -> TextureWrap {
    if bits & 1 != 0 {
        TextureWrap::Clamp
    } else if bits & 2 != 0 {
        TextureWrap::Mirror
    } else {
        TextureWrap::Repeat
    }
}

fn upload(model: &Model, attr: &TexAttr) -> Option<Texture2D> {
    if attr.image & 0x8000 != 0 {
        return None;
    }
    let img = model.textures.get(attr.image as usize)?.decode().ok()?;
    let tex = Texture2D::from_rgba8(img.width as u16, img.height as u16, &img.rgba);
    tex.set_filter(FilterMode::Linear);
    // SAFETY: only called on the main thread during loading, as macroquad requires.
    let gl = unsafe { macroquad::window::get_internal_gl() };
    gl.quad_context.texture_set_wrap(
        tex.raw_miniquad_id(),
        wrap(attr.tiling),
        wrap(attr.tiling >> 8),
    );
    Some(tex)
}

impl GpuModel {
    fn load(assets: &Assets, path: &str) -> Result<Self, libpikmin_assets::Error> {
        let model = assets.model(path)?;
        let skel = Skeleton::new(&model);
        let mut cache: HashMap<usize, Option<Texture2D>> = HashMap::new();
        let mut tex = |a: usize| {
            cache
                .entry(a)
                .or_insert_with(|| model.tex_attrs.get(a).and_then(|ta| upload(&model, ta)))
                .clone()
        };
        let materials = model
            .materials
            .iter()
            .map(|m| {
                let pvw = m.flags & mat_flags::PVW != 0;
                let tev = m
                    .tev_info
                    .and_then(|t| model.tev_infos.get(t))
                    .filter(|_| pvw);
                let textures: [Option<Texture2D>; 4] = if pvw {
                    std::array::from_fn(|i| m.tex_datas.get(i).and_then(|d| tex(d.tex_attr)))
                } else {
                    [m.tex_attr.and_then(&mut tex), None, None, None]
                };
                let program = match tev {
                    Some(t) if !t.stages.is_empty() => Program::new(&t.stages),
                    _ => Program::modulate(textures[0].is_some()),
                };
                GpuMaterial {
                    program,
                    textures,
                    blend: m.blend(),
                    lit: m.lit(),
                    specular: pvw && m.lighting & 2 != 0,
                    vertex_color: m.vertex_color(),
                }
            })
            .collect();
        let batches = model.batches()?;
        let pose = skel.pose(Mat4::IDENTITY, None);
        let (mut lo, mut hi) = (GVec3::splat(f32::MAX), GVec3::splat(f32::MIN));
        for b in &batches {
            for v in &b.vertices {
                let p = pose.skin(v).0;
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        Ok(GpuModel {
            model,
            skel,
            batches,
            materials,
            lo,
            hi,
        })
    }

    /// Horizontal radius of the bind pose.
    fn radius(&self) -> f32 {
        [self.lo.x, self.lo.z, self.hi.x, self.hi.z]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()))
            .max(0.01)
    }
}

/// Per-draw material overrides.
#[derive(Default, Clone, Copy)]
struct Look {
    /// Replaces material 0's colour (`ViewPiki` tints `mMaterialList[0]` with the Pikmin's
    /// colour).
    tint0: Option<[u8; 4]>,
    /// Replaces TEV colour register 0's RGB (pellet colour).
    reg0: Option<[u8; 4]>,
    /// Frame for material and register colour animations (`GoalItem::refresh` picks the
    /// Onion's colour with frame = colour * 2).
    color_frame: Option<f32>,
    /// Multiplies the output colour (damage flash).
    brightness: Option<f32>,
    /// Multiplies alpha (fading out).
    alpha: Option<f32>,
    /// Draw with lighting off (`gfx.setLighting(false)`).
    unlit: bool,
}

/// Scene-wide state for drawing models.
struct Ctx<'a> {
    tev: &'a mut Tev,
    time: f32,
    /// Direction from the scene towards the camera, for specular.
    to_eye: GVec3,
}

const LIGHT: GVec3 = GVec3::new(0.35, 0.85, 0.40);
const AMBIENT: f32 = 0.45;

/// Rasterized colour channel 0 (diffuse) and channel 1's specular term.
fn light(c: [u8; 4], normal: GVec3, gm: &GpuMaterial, to_eye: GVec3, unlit: bool) -> ([u8; 4], f32) {
    let n = normal.normalize_or_zero();
    let l = LIGHT.normalize();
    let k = if gm.lit && !unlit {
        AMBIENT + (1.0 - AMBIENT) * n.dot(l).max(0.0)
    } else {
        1.0
    };
    let spec = if gm.specular {
        n.dot((l + to_eye).normalize()).max(0.0).powf(24.0) * 0.6
    } else {
        0.0
    };
    (
        [
            (c[0] as f32 * k).min(255.0) as u8,
            (c[1] as f32 * k).min(255.0) as u8,
            (c[2] as f32 * k).min(255.0) as u8,
            c[3],
        ],
        spec,
    )
}

/// Skins and draws one model. Returns the posed joint matrices (for attachments).
fn draw_model(
    gm: &GpuModel,
    root: Mat4,
    anim: Option<(&Anim, f32)>,
    look: Look,
    ctx: &mut Ctx,
) -> Vec<Mat4> {
    let pose = gm.skel.pose(root, anim);
    draw_pose(gm, pose, look, &|_| look.color_frame, ctx)
}

/// Draws a model in `pose`, with each material's colour animations at `frame(material)`
/// (`ShapeDynMaterials` lets parts of a model animate their materials apart).
fn draw_pose(
    gm: &GpuModel,
    pose: libpikmin_assets::Pose,
    look: Look,
    frame: &dyn Fn(usize) -> Option<f32>,
    ctx: &mut Ctx,
) -> Vec<Mat4> {
    let colors: Vec<[u8; 4]> = gm
        .model
        .materials
        .iter()
        .enumerate()
        .map(|(i, m)| match look.tint0 {
            Some(t) if i == 0 => t,
            _ => m.color_at(frame(i)),
        })
        .collect();

    for pass in [Blend::Opaque, Blend::AlphaTest, Blend::AlphaBlend] {
        for b in &gm.batches {
            let gmat = &gm.materials[b.material];
            let in_pass = match pass {
                Blend::AlphaBlend => matches!(gmat.blend, Blend::AlphaBlend | Blend::Inverse),
                p => gmat.blend == p,
            };
            if !in_pass {
                continue;
            }
            let vertices: Vec<Vertex> = b
                .vertices
                .iter()
                .map(|v| {
                    let (p, n) = pose.skin(v);
                    let base = if gmat.vertex_color {
                        v.color
                    } else {
                        colors[b.material]
                    };
                    let (c, spec) = light(base, n, gmat, ctx.to_eye, look.unlit);
                    Vertex {
                        position: mq(p),
                        uv: vec2(v.uv.x, v.uv.y),
                        color: c,
                        normal: vec4(n.x, n.y, n.z, spec),
                    }
                })
                .collect();
            let indices: Vec<u16> = b.indices.iter().map(|&i| i as u16).collect();
            let m = &gm.model.materials[b.material];
            let info = m.tev_info.and_then(|t| gm.model.tev_infos.get(t));
            let mut regs = info
                .map(|t| t.regs.each_ref().map(|r| r.color_at(frame(b.material))))
                .unwrap_or([[255.0; 4]; 3]);
            if let Some(c) = look.reg0 {
                regs[0][..3].copy_from_slice(&[c[0] as f32, c[1] as f32, c[2] as f32]);
            }
            let params = DrawParams {
                regs,
                konst: info.map(|t| t.konst).unwrap_or([[255; 4]; 4]),
                gens: tev::gen_params(&gm.model, b.material, ctx.time),
                textures: gmat.textures.clone(),
                blend: gmat.blend,
                cull: b.cull,
                brightness: look.brightness.unwrap_or(1.0),
                alpha: look.alpha.unwrap_or(1.0),
            };
            ctx.tev.draw(
                &gmat.program,
                &params,
                Mesh {
                    vertices,
                    indices,
                    texture: None,
                },
            );
        }
    }
    pose.joints
}

/// How an animation's frame is driven.
#[derive(Clone, Copy)]
enum Clock {
    /// Use the sim's own counter.
    Sim(f32),
    /// Loop over the anim keys at this many frames per second.
    Loop(f32),
    /// Play once and hold the last frame.
    Once(f32),
    /// Start at this frame, then loop over the motion's loop keys (`Pick` while carrying:
    /// `ActTransport` starts it at 11).
    LoopFrom(f32, f32),
}

#[derive(Clone, Copy)]
struct Player {
    /// AnimMgr index being played.
    anim: usize,
    frame: f32,
}

fn advance<'a>(
    players: &mut HashMap<u64, Player>,
    key: u64,
    set: &'a AnimSet,
    motion: Motion,
    clock: Clock,
    dt: f32,
) -> Option<(&'a Anim, f32)> {
    advance_index(players, key, set, motion.anim_index()?, clock, dt)
}

/// Like [`advance`] for sets without a motion table (items, tekis): `index` is the AnimMgr
/// entry itself.
fn advance_index<'a>(
    players: &mut HashMap<u64, Player>,
    key: u64,
    set: &'a AnimSet,
    index: usize,
    clock: Clock,
    dt: f32,
) -> Option<(&'a Anim, f32)> {
    let (info, anim) = set.get(index)?;
    let start = *info.keys.first().unwrap_or(&0) as f32;
    let end = (*info.keys.last().unwrap_or(&0) as f32).max(start);
    let first = match clock {
        Clock::LoopFrom(_, from) => from,
        _ => start,
    };
    let p = players.entry(key).or_insert(Player {
        anim: index,
        frame: first,
    });
    if p.anim != index {
        *p = Player {
            anim: index,
            frame: first,
        };
    }
    match clock {
        Clock::Sim(f) => p.frame = f,
        Clock::Loop(fps) => {
            p.frame += fps * dt;
            if end > start && p.frame >= end {
                p.frame = start + (p.frame - start) % (end - start);
            }
        }
        Clock::Once(fps) => p.frame = (p.frame + fps * dt).min(end),
        Clock::LoopFrom(fps, _) => {
            let key = |ty: u8| {
                let (n, _) = info.info_keys.iter().find(|(_, t)| *t == ty)?;
                info.keys.get(*n as usize).map(|&f| f as f32)
            };
            let (lo, hi) = key(0).zip(key(1)).unwrap_or((start, end));
            p.frame += fps * dt;
            if hi > lo && p.frame >= hi {
                p.frame = lo + (p.frame - lo) % (hi - lo);
            }
        }
    }
    let last = anim.frames.saturating_sub(1) as f32;
    Some((anim, p.frame.clamp(0.0, last)))
}

/// `updateWalkAnimation`: WAIT, ASIBUMI, WALK, RUN or NIGERU by speed, with the walk/run
/// rate scaled between its min and max.
fn locomotion(speed: f32, th: [f32; 4], rate: [f32; 5]) -> (Motion, Clock) {
    let ramp = |lo: f32, hi: f32, a: f32, b: f32| a + (b - a) * ((speed - lo) / (hi - lo)).clamp(0.0, 1.0);
    if speed < th[0] {
        (Motion::Wait, Clock::Loop(30.0))
    } else if speed < th[1] {
        (Motion::Asibumi, Clock::Loop(30.0))
    } else if speed < th[2] {
        (Motion::Walk, Clock::Loop(ramp(th[1], th[2], rate[0], rate[1])))
    } else if speed < th[3] {
        (Motion::Run, Clock::Loop(ramp(th[2], th[3], rate[2], rate[3])))
    } else {
        (Motion::Nigeru, Clock::Loop(rate[4]))
    }
}

fn piki_rgb(c: PikiColor) -> [u8; 4] {
    // `Piki::pikiColors` (piki.cpp).
    match c {
        PikiColor::Blue => [0, 50, 255, 255],
        PikiColor::Red => [255, 30, 0, 255],
        PikiColor::Yellow => [255, 210, 0, 255],
    }
}

/// `PikiHeadMotion` (PikiHeadItem.h): indices into `objects/pikihead/anims.bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PikiHeadMotion {
    TaneFall = 0,
    TaneUp = 1,
    Hatuga = 2,
    UgWait = 3,
    UgGrow = 4,
    Sioreru = 5,
    TaneDown = 6,
}

/// What the viewer remembers about a sprout to pick its motion.
struct SproutTrack {
    state: std::mem::Discriminant<SproutState>,
    happa: libpikmin::Happa,
    motion: PikiHeadMotion,
}

/// A ship part's model, its file name (for the ship joint), `PelletConfig` p10 scale, and
/// its animations with the joints its upper animator drives (`PelletShapeObject`: the subtree
/// under `mOverrideJoint`).
pub struct ShipPartModel {
    model: GpuModel,
    pub file: String,
    pub scale: f32,
    anims: Option<AnimSet>,
    override_joint: i32,
    upper: Vec<bool>,
}

/// The S.S. Dolphin's model, animations and animation state.
struct ShipView {
    model: GpuModel,
    anims: AnimSet,
    /// Which `UfoAnimator` context drives each joint.
    ctx_of_joint: Vec<usize>,
    /// Which level light animates each material (`LightAnimator`'s `ShapeDynMaterials`).
    light_of_material: Vec<Option<usize>>,
    anim: ShipAnimator,
    started: bool,
    /// Onions (maybe the ship) a pellet reached this frame, and ship parts that went in.
    arrivals: Vec<OnionId>,
    collected: Vec<(u32, u32)>,
    /// Onions (maybe the ship) back on the ground this frame.
    landed: Vec<OnionId>,
    /// Gone after taking off.
    away: bool,
    /// Running flames: their slot and the collision sphere they follow.
    flames: Vec<(u8, &'static str)>,
}

impl ShipView {
    fn load(assets: &Assets, parts: &[Option<ShipPartModel>]) -> Option<Self> {
        use libpikmin_assets::ship;
        let model = GpuModel::load(assets, ship::SHIP_MODEL).ok()?;
        let anims = AnimSet::load(assets, ship::SHIP_ANIM_MGR, ship::SHIP_BUNDLE).ok()?;
        let ctx_of_joint = model.skel.override_map(&ship::CONTEXT_JOINTS);
        let mut light_of_material = vec![None; model.model.materials.len()];
        for (l, &root) in ship::LIGHT_JOINTS.iter().enumerate() {
            for j in model.skel.subtree(root) {
                for &(m, _) in &model.model.joints[j].matpolys {
                    if let Some(slot) = light_of_material.get_mut(m as usize) {
                        *slot = Some(l);
                    }
                }
            }
        }
        let part_mgrs = parts
            .iter()
            .map(|p| {
                let p = p.as_ref()?;
                Some((p.anims.as_ref()?.mgr.clone(), p.override_joint >= 0))
            })
            .collect();
        let anim = ShipAnimator::new(&anims.mgr, part_mgrs);
        Some(ShipView {
            model,
            anims,
            ctx_of_joint,
            light_of_material,
            anim,
            started: false,
            arrivals: Vec::new(),
            collected: Vec::new(),
            landed: Vec::new(),
            away: false,
            flames: Vec::new(),
        })
    }
}

/// Loads every ship part's model (`pelAnim.bin` → `objects/<folder>/<file>.mod`).
fn ship_part_models(assets: &Assets) -> Vec<Option<ShipPartModel>> {
    use libpikmin_assets::ship;
    let infos = assets
        .read(ship::PEL_ANIM)
        .ok()
        .and_then(|d| ship::parse_pel_anim(&d).ok())
        .unwrap_or_default();
    let configs = assets
        .read("parms/pelMgr.bin")
        .ok()
        .and_then(|d| libpikmin::pellet_config::parse_pellet_configs(&d).ok())
        .unwrap_or_default();
    ship::PART_IDS
        .iter()
        .map(|id| {
            let info = infos.iter().find(|i| i.id == *id)?;
            let model = GpuModel::load(assets, &info.model()).ok()?;
            let scale = libpikmin::pellet_config::find(&configs, id).map_or(1.0, |c| c.scale);
            let anims = AnimSet::load(assets, &info.anim_mgr(), &info.bundle()).ok();
            let mut upper = vec![false; model.skel.len()];
            if info.override_joint >= 0 {
                for j in model.skel.subtree(info.override_joint as usize) {
                    upper[j] = true;
                }
            }
            Some(ShipPartModel {
                model,
                file: info.file.clone(),
                scale,
                anims,
                override_joint: info.override_joint,
                upper,
            })
        })
        .collect()
}

/// Text or a bar to draw in screen space after the 3D pass.
pub enum Overlay {
    Text { at: Vec3, text: String, color: Color, size: f32 },
    Bar { at: Vec3, width: f32, frac: f32, color: Color },
}

pub struct Scene3d {
    olimar: GpuModel,
    pikmin: [GpuModel; 3],
    happa: [GpuModel; 3],
    onion: GpuModel,
    /// `shapes/cursor/`: the cursor and the marker under it (`Navi::refresh`).
    cursor: Option<(GpuModel, GpuModel)>,
    /// The whistle's particles.
    effects: Option<Effects>,
    /// The S.S. Dolphin (`ufo0705`) and its animations.
    ship: Option<ShipView>,
    /// Ship sounds to start (true) or stop this frame (`playEventSound(ufo, ...)`).
    pub ship_sounds: Vec<(OnionId, &'static str, bool)>,
    /// Commands for the cutscene sound track this frame (`Jac_DemoSound`: demo, command);
    /// `None` ends it.
    pub demo_sounds: Vec<Option<(u16, u16)>>,
    /// The game is paused (a message is up): the ship's animations hold.
    pub paused: bool,
    /// Ship part models by part index (`_ufoIDTable`), with their file names
    /// (`pelAnim.bin`) and `PelletConfig` scale.
    parts: Vec<Option<ShipPartModel>>,
    pellets: [GpuModel; 4],
    pikihead: GpuModel,
    pikihead_anims: AnimSet,
    /// Enemy model and animations by `TekiKind::index`, if present on the disc.
    tekis: Vec<Option<(GpuModel, AnimSet)>>,
    puddle: GpuModel,
    puddle_anims: AnimSet,
    drop: GpuModel,
    drop_anims: AnimSet,
    sprouts: HashMap<libpikmin::onion::SproutId, SproutTrack>,
    /// Sprout animation sound keys hit this frame: (parent Onion, value, position).
    pub sprout_sounds: Vec<(Option<libpikmin::onion::OnionId>, u8, Vec3)>,
    piki_anims: AnimSet,
    onion_anims: AnimSet,
    tev: Tev,
    time: f32,
    ground: Texture2D,
    players: HashMap<u64, Player>,
    throw_timer: HashMap<CaptainId, f32>,
    /// Camera: direction it looks (decomp angle convention) and distance/pitch.
    pub yaw: f32,
    pub dist: f32,
    pub pitch: f32,
    pub camera: Camera3D,
}

impl Scene3d {
    pub fn load(assets: &Assets) -> Result<Self, libpikmin_assets::Error> {
        let m = |p: &str| GpuModel::load(assets, p);
        let parts = ship_part_models(assets);
        let ground = {
            // A procedural grass checker (not game data).
            let n = 64usize;
            let mut rgba = vec![0u8; n * n * 4];
            for y in 0..n {
                for x in 0..n {
                    let checker = ((x / 32) + (y / 32)) % 2 == 0;
                    let noise = ((x * 7 + y * 13) ^ (x * y)) % 9;
                    let (r, g, b) = if checker { (64, 104, 50) } else { (58, 96, 46) };
                    let i = (y * n + x) * 4;
                    rgba[i..i + 4].copy_from_slice(&[
                        r + noise as u8,
                        g + noise as u8,
                        b + noise as u8 / 2,
                        255,
                    ]);
                }
            }
            let t = Texture2D::from_rgba8(n as u16, n as u16, &rgba);
            let gl = unsafe { macroquad::window::get_internal_gl() };
            gl.quad_context
                .texture_set_wrap(t.raw_miniquad_id(), TextureWrap::Repeat, TextureWrap::Repeat);
            t
        };
        Ok(Scene3d {
            olimar: m(paths::OLIMAR)?,
            pikmin: [m(paths::PIKMIN[0])?, m(paths::PIKMIN[1])?, m(paths::PIKMIN[2])?],
            happa: [m(paths::HAPPA[0])?, m(paths::HAPPA[1])?, m(paths::HAPPA[2])?],
            onion: m(paths::ONION)?,
            cursor: m(paths::CURSOR).ok().zip(m(paths::CURSOR_MARKER).ok()),
            effects: Effects::load(assets),
            ship: ShipView::load(assets, &parts),
            ship_sounds: Vec::new(),
            demo_sounds: Vec::new(),
            paused: false,
            parts,
            pellets: [
                m(paths::PELLETS[0])?,
                m(paths::PELLETS[1])?,
                m(paths::PELLETS[2])?,
                m(paths::PELLETS[3])?,
            ],
            pikihead: m(paths::PIKIHEAD)?,
            tekis: TekiKind::ALL
                .iter()
                .map(|k| {
                    let [model, anm, key, _] = libpikmin_assets::teki::teki_paths(k.name());
                    Some((m(&model).ok()?, AnimSet::load(assets, &key, &anm).ok()?))
                })
                .collect(),
            puddle: m(paths::WATER)?,
            puddle_anims: AnimSet::load(assets, paths::WATER_ANIM_MGR, paths::WATER_BUNDLE)?,
            drop: m(paths::FALL_WATER)?,
            drop_anims: AnimSet::load(assets, paths::FALL_WATER_ANIM_MGR, paths::FALL_WATER_BUNDLE)?,
            pikihead_anims: AnimSet::load(assets, paths::PIKIHEAD_ANIM_MGR, paths::PIKIHEAD_BUNDLE)?,
            sprouts: HashMap::new(),
            sprout_sounds: Vec::new(),
            piki_anims: AnimSet::load(assets, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE)?,
            onion_anims: AnimSet::load(assets, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE)?,
            tev: Tev::new(),
            time: 0.0,
            ground,
            players: HashMap::new(),
            throw_timer: HashMap::new(),
            yaw: 0.0,
            dist: 420.0,
            pitch: 0.72,
            camera: Camera3D::default(),
        })
    }

    pub fn on_event(&mut self, e: &Event) {
        match e {
            Event::Thrown { captain, .. } => {
                self.throw_timer.insert(*captain, 9.0 / 30.0);
            }
            Event::PelletArrived { onion, .. } => {
                if let Some(s) = self.ship.as_mut() {
                    s.arrivals.push(*onion);
                }
            }
            Event::ShipPartCollected { part, total, .. } => {
                if let Some(s) = self.ship.as_mut() {
                    s.collected.push((*part, *total));
                }
            }
            Event::OnionLanded { onion } => {
                if let Some(s) = self.ship.as_mut() {
                    s.landed.push(*onion);
                }
            }
            // `Piki::doKill`: the ghost rises from where it died.
            Event::PikiGone { pos, .. } => {
                if let Some(fx) = self.effects.as_mut() {
                    fx.spawn(libpikmin_assets::particle::effects::PIKI_DEAD_SOUL, *pos);
                }
            }
            _ => {}
        }
    }

    /// Turns a stick in screen terms (x right, y away from the camera) into the sim's
    /// world-XZ stick.
    pub fn camera_stick(&self, s: Vec2) -> Vec2 {
        let (fx, fz) = (self.yaw.sin(), self.yaw.cos());
        // Right of a camera looking along f in a right-handed, Y-up world: f x Y = (-fz, fx).
        Vec2::new(s.x * -fz + s.y * fx, s.x * fx + s.y * fz)
    }

    fn update_camera(&mut self, target: Vec3) {
        let f = GVec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
        let t = target + GVec3::new(0.0, 15.0, 0.0);
        let eye = t - f * self.dist * self.pitch.cos() + GVec3::Y * self.dist * self.pitch.sin();
        self.camera = Camera3D {
            position: mq(eye),
            target: mq(t),
            up: vec3(0.0, 1.0, 0.0),
            fovy: 40f32.to_radians(),
            z_near: 5.0,
            z_far: 20000.0,
            ..Default::default()
        };
    }

    /// The camera's world-to-view matrix, for positional sound.
    pub fn view_matrix(&self) -> Mat4 {
        let (e, t) = (self.camera.position, self.camera.target);
        Mat4::look_at_rh(GVec3::new(e.x, e.y, e.z), GVec3::new(t.x, t.y, t.z), GVec3::Y)
    }

    pub fn world_to_screen(&self, p: Vec3) -> Option<macroquad::math::Vec2> {
        let clip = self.camera.matrix() * vec4(p.x, p.y, p.z, 1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip / clip.w;
        Some(vec2(
            (ndc.x + 1.0) * 0.5 * screen_width(),
            (1.0 - ndc.y) * 0.5 * screen_height(),
        ))
    }

    fn ring(centre: Vec3, radius: f32, color: Color) {
        let n = 40;
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            let p = |a: f32| vec3(centre.x + a.sin() * radius, centre.y + 0.6, centre.z + a.cos() * radius);
            draw_line_3d(p(a0), p(a1), color);
        }
    }

    /// Draws the whole scene in 3D, including the host's enemies.
    pub fn draw(
        &mut self,
        sim: &Sim,
        focus: CaptainId,
        tekis: &Tekis,
        dt: f32,
    ) -> Vec<Overlay> {
        let params: Params = sim.params().clone();
        // `PIKMIN_FOCUS=x,y,z` pins the camera target (for screenshots).
        let target = std::env::var("PIKMIN_FOCUS")
            .ok()
            .and_then(|v| {
                let p: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                (p.len() == 3).then(|| Vec3::new(p[0], p[1], p[2]))
            })
            .unwrap_or(sim.captain(focus).pos);
        self.update_camera(target);
        set_camera(&self.camera);
        let mut overlays = Vec::new();
        for t in self.throw_timer.values_mut() {
            *t -= dt;
        }

        self.draw_ground(sim.captain(focus).pos);
        self.time += dt;
        let (eye, target) = (self.camera.position, self.camera.target);
        self.tev.view_rot = macroquad::math::Mat4::look_at_rh(eye, target, vec3(0.0, 1.0, 0.0));
        let d = target - eye;
        let mut ctx = Ctx {
            tev: &mut self.tev,
            time: self.time,
            to_eye: -GVec3::new(d.x, d.y, d.z).normalize_or_zero(),
        };

        // The ship (`UfoItem`): eight animation contexts each drive part of its skeleton
        // (`UfoAnimator`), the level lights animate their materials (`LightAnimator`), and the
        // parts on board play their own motions on their `null_<part>` joints
        // (`PlayerState::renderParts`).
        if let Some(sv) = self.ship.as_mut()
            && let Some(o) = sim.onions().iter().find(|o| o.ship.is_some())
            && let Some(ship) = o.ship.as_ref()
        {
            use libpikmin_assets::ship::{ShipCue, ship_joint};
            const SHIP_FX: u64 = 6 << 40;
            let mut cues = Vec::new();
            let level = params.ai.ship_level(ship.parts.len() as u32);
            if !sv.started {
                sv.started = true;
                sv.anim.start(&ship.parts, level, &mut cues);
            }
            // The next day (`UfoItem::startAI`).
            if std::mem::take(&mut sv.landed).contains(&o.id) {
                sv.anim.land(&ship.parts, level, &mut cues);
                sv.away = false;
            }
            // `DemoEventUFO::Takeoff`
            if matches!(o.anim, OnionAnim::TakeOff { .. }) && !sv.anim.taking_off() {
                sv.anim.take_off(&ship.parts, &mut cues);
            }
            let away = o.anim == OnionAnim::Away;
            if away && !sv.away {
                // Out of sight: its flames and the take off's sound stop.
                sv.away = true;
                sv.flames.clear();
                if let Some(fx) = self.effects.as_mut() {
                    fx.detach_where(|k| k & !0xFF == SHIP_FX);
                }
                self.demo_sounds.push(None);
            }
            let yaw = {
                let d = ship.goal - o.pos;
                d.x.atan2(d.z)
            };
            // `UfoItem::suckMe` when a part reaches the ship.
            for onion in std::mem::take(&mut sv.arrivals) {
                if onion == o.id {
                    sv.anim.suck_start();
                }
            }
            for (part, total) in std::mem::take(&mut sv.collected) {
                sv.anim.suck_finish(part as usize, params.ai.ship_level(total), &mut cues);
                // `finishSuck`: a puff where it went in, blown the way the ship faces.
                if let Some(fx) = self.effects.as_mut()
                    && let Some(g) = fx.spawn(libpikmin_assets::particle::effects::ROCKET_GEP, o.suck_pos())
                {
                    g.emit_dir = GVec3::new(yaw.sin(), 0.0, yaw.cos());
                }
            }
            if !away {
            sv.anim.update(if self.paused { 0.0 } else { dt }, &mut cues);

            let root = srt_matrix(GVec3::ONE, GVec3::new(0.0, yaw, 0.0), o.pos);
            let (anim, anims, map) = (&sv.anim, &sv.anims, &sv.ctx_of_joint);
            let pose = sv.model.skel.pose_by(root, |j| {
                let (m, f) = anim.context(map.get(j).copied().unwrap_or(0));
                let (_, a) = anims.get(m)?;
                Some((a, f.clamp(0.0, a.frames.saturating_sub(1) as f32)))
            });
            let lights = &sv.light_of_material;
            let frame = |m: usize| Some(lights.get(m).copied().flatten().map_or(0.0, |l| anim.light_frame(l)));
            let joints = draw_pose(&sv.model, pose, Look::default(), &frame, &mut ctx);
            let part_joint = |part: usize| {
                let pm = self.parts.get(part)?.as_ref()?;
                joints.get(sv.model.model.joint_index(&ship_joint(&pm.file))?).copied()
            };
            for &part in &ship.parts {
                let (Some(pa), Some(jm)) = (anim.part(part as usize), part_joint(part as usize)) else { continue };
                let Some(Some(pm)) = self.parts.get(part as usize) else { continue };
                let pose = match pm.anims.as_ref() {
                    Some(set) => pm.model.skel.pose_by(jm, |k| {
                        let p = if pm.upper.get(k).copied().unwrap_or(false) { pa.upper.as_ref()? } else { &pa.lower };
                        let (_, a) = set.get(p.motion)?;
                        Some((a, p.frame.clamp(0.0, a.frames.saturating_sub(1) as f32)))
                    }),
                    None => pm.model.skel.pose(jm, None),
                };
                draw_pose(&pm.model, pose, Look::default(), &|_| None, &mut ctx);
            }
            // A collision sphere's centre ('eng1', 'gcen', ...).
            let coll = &sv.model.model.coll;
            let sphere = |id: &str| {
                let c = coll.iter().find(|c| c.id == id)?;
                Some(joints.get(c.joint)?.transform_point3(c.centre))
            };
            for c in cues {
                let (fx, at, normal) = match c {
                    ShipCue::Sound(name) => {
                        self.ship_sounds.push((o.id, name, true));
                        continue;
                    }
                    ShipCue::StopSound(name) => {
                        self.ship_sounds.push((o.id, name, false));
                        continue;
                    }
                    ShipCue::DemoSound { demo, cmd } => {
                        self.demo_sounds.push(Some((demo, cmd)));
                        continue;
                    }
                    ShipCue::Attached { fx, sphere: id, normal, slot } => {
                        let Some(at) = sphere(id) else { continue };
                        if let Some(e) = self.effects.as_mut()
                            && let Some(g) = e.attach(SHIP_FX | slot as u64, fx, at)
                            && let Some(n) = normal
                        {
                            g.oriented_normal = n;
                        }
                        sv.flames.retain(|f| f.0 != slot);
                        sv.flames.push((slot, id));
                        continue;
                    }
                    ShipCue::JointEffect { fx, joint, offset, dir, normal } => {
                        let Some(m) = joints.get(joint) else { continue };
                        if let Some(e) = self.effects.as_mut()
                            && let Some(g) = e.spawn(fx, m.transform_point3(offset))
                        {
                            g.emit_dir = root.transform_vector3(dir);
                            if let Some(n) = normal {
                                g.oriented_normal = n;
                            }
                        }
                        continue;
                    }
                    ShipCue::Effect { fx, lift, normal } => (fx, o.pos + GVec3::Y * lift, normal),
                    ShipCue::PartEffect { fx, part } => {
                        let Some(jm) = part_joint(part) else { continue };
                        (fx, jm.w_axis.truncate(), None)
                    }
                };
                if let Some(e) = self.effects.as_mut()
                    && let Some(g) = e.spawn(fx, at)
                    && let Some(n) = normal
                {
                    g.oriented_normal = n;
                }
            }
            if let Some(e) = self.effects.as_mut() {
                for &(slot, id) in &sv.flames {
                    if let Some(at) = sphere(id) {
                        e.move_attached(SHIP_FX | slot as u64, at);
                    }
                }
            }
            }
        }

        // Onions.
        for (i, o) in sim.onions().iter().enumerate() {
            if o.ship.is_some() {
                continue;
            }
            // `GoalItem::playEffect` keeps its flying sparkles under this key.
            let fx_key = (5u64 << 40) | i as u64;
            // `GoalItem::mOnionColour` order (blue, red, yellow) picks the flight motions.
            let colour = o.colors[0].index();
            let (motion_idx, clock) = match o.anim {
                OnionAnim::Idle => (1, Clock::Loop(30.0)),
                OnionAnim::Suction { frame } => (2, Clock::Sim(frame)),
                OnionAnim::Emitting { frame } => (0, Clock::Sim(frame)),
                OnionAnim::TakeOff { frame } => (libpikmin::anim::ANIM_GOAL_TAKEOFF + colour, Clock::Sim(frame)),
                OnionAnim::Landing { frame } => (libpikmin::anim::ANIM_GOAL_LAND + colour, Clock::Sim(frame)),
                OnionAnim::Away => {
                    if let Some(e) = self.effects.as_mut() {
                        e.detach(fx_key);
                    }
                    continue;
                }
            };
            let key = (3u64 << 40) | i as u64;
            // Where the motion was last frame, for its effect keys.
            let before = self.players.get(&key).filter(|p| p.anim == motion_idx).map(|p| p.frame);
            let anim = self.onion_anims.get(motion_idx).map(|(info, a)| {
                let p = self.players.entry(key).or_insert(Player {
                    anim: motion_idx,
                    frame: 0.0,
                });
                if p.anim != motion_idx {
                    *p = Player { anim: motion_idx, frame: 0.0 };
                }
                let end = *info.keys.last().unwrap_or(&0) as f32;
                p.frame = match clock {
                    Clock::Sim(f) => f,
                    _ => (p.frame + 30.0 * dt) % end.max(1.0),
                };
                (a, p.frame.min(a.frames.saturating_sub(1) as f32))
            });
            let root = srt_matrix(GVec3::ONE, GVec3::ZERO, o.pos);
            let now = anim.map_or(0.0, |(_, f)| f);
            let joints = draw_model(
                &self.onion,
                root,
                anim,
                Look {
                    color_frame: Some(colour as f32 * 2.0),
                    ..Default::default()
                },
                &mut ctx,
            );
            let flying = matches!(o.anim, OnionAnim::TakeOff { .. } | OnionAnim::Landing { .. });
            if let (true, Some(e), Some((info, _))) = (flying, self.effects.as_mut(), self.onion_anims.get(motion_idx)) {
                let coll = &self.onion.model.coll;
                let sphere = |id: &str| {
                    let c = coll.iter().find(|c| c.id == id)?;
                    Some(joints.get(c.joint)?.transform_point3(c.centre))
                };
                let landing = matches!(o.anim, OnionAnim::Landing { .. });
                // `GoalItem::startLand` turns the sparkles on.
                if landing
                    && before.is_none()
                    && let Some(at) = sphere("piki")
                {
                    e.attach(fx_key, effects::KAFUN_NG, at);
                }
                let from = before.unwrap_or(-1.0);
                for k in info.event_keys.iter().filter(|k| k.event == 1) {
                    let f = k.frame as f32;
                    if !(from <= f && f < now) {
                        continue;
                    }
                    // `GoalItem::playEffect`. Not shown: the light cone shrinking (this viewer
                    // has no cone) and the flight light, which the game only records.
                    match (landing, k.value) {
                        (false, 0) => {
                            if let Some(at) = sphere("piki") {
                                e.attach(fx_key, effects::KAFUN_NG, at);
                            }
                        }
                        (false, 2) => {
                            e.spawn(effects::ROCKET_TAKE_S, o.pos);
                        }
                        (true, 0) => {
                            e.spawn(effects::ROCKET_LAND_S, o.pos);
                        }
                        (true, 1 | 2) => {
                            let feet: &[&str] = if k.value == 1 { &["bas1"] } else { &["bas1", "bas2", "bas3"] };
                            for at in feet.iter().filter_map(|id| sphere(id)) {
                                e.spawn(effects::ROCKET_SMOKE_D, at);
                            }
                        }
                        (true, 3) => e.detach(fx_key),
                        _ => {}
                    }
                }
                if let Some(at) = sphere("piki") {
                    e.move_attached(fx_key, at);
                }
            }
        }

        // Pellets and corpses: the numbered pellets, sized to the thing being carried.
        for pel in sim.pellets() {
            if pel.state == PelletState::Gone {
                continue;
            }
            if pel.desc.creature.is_some() {
                if !matches!(pel.state, PelletState::Sucking { .. }) {
                    overlays.push(Overlay::Text {
                        at: pel.pos + GVec3::new(0.0, pel.desc.height + 20.0, 0.0),
                        text: format!("{}/{}", pel.carriers.len(), pel.desc.min_carriers),
                        color: Color::new(1.0, 1.0, 1.0, 1.0),
                        size: 18.0,
                    });
                }
                continue;
            }
            // Ship parts: their own models at the config's scale.
            if let Some(Some(pm)) = pel.desc.ship_part.map(|p| self.parts.get(p as usize).and_then(Option::as_ref)) {
                let s = pm.scale * pel.scale;
                let root = srt_matrix(
                    GVec3::splat(s),
                    GVec3::new(0.0, pel.yaw, 0.0),
                    pel.pos - GVec3::new(0.0, pm.model.lo.y * s, 0.0),
                );
                draw_model(&pm.model, root, None, Look::default(), &mut ctx);
                if !matches!(pel.state, PelletState::Sucking { .. }) {
                    overlays.push(Overlay::Text {
                        at: pel.pos + GVec3::new(0.0, (pm.model.hi.y - pm.model.lo.y) * s + 10.0, 0.0),
                        text: format!("{}/{}", pel.carriers.len(), pel.desc.min_carriers),
                        color: Color::new(1.0, 1.0, 1.0, 1.0),
                        size: 18.0,
                    });
                }
                continue;
            }
            let idx = match pel.desc.min_carriers {
                20.. => 3,
                10.. => 2,
                5.. => 1,
                _ => 0,
            };
            let gm = &self.pellets[idx];
            let s = pel.desc.radius / gm.radius() * pel.scale;
            let root = srt_matrix(
                GVec3::splat(s),
                GVec3::new(0.0, pel.yaw, 0.0),
                pel.pos - GVec3::new(0.0, gm.lo.y * s, 0.0),
            );
            let tint = pel.desc.color.map(piki_rgb).unwrap_or([230, 230, 230, 255]);
            draw_model(
                gm,
                root,
                None,
                Look {
                    reg0: Some(tint),
                    ..Default::default()
                },
                &mut ctx,
            );
            if !matches!(pel.state, PelletState::Sucking { .. }) {
                overlays.push(Overlay::Text {
                    at: pel.pos + GVec3::new(0.0, (gm.hi.y - gm.lo.y) * s + 8.0, 0.0),
                    text: format!("{}/{}", pel.carriers.len(), pel.desc.min_carriers),
                    color: Color::new(1.0, 1.0, 1.0, 1.0),
                    size: 18.0,
                });
            }
        }

        // Sprouts and flying seeds: the pikihead item (`PikiHeadItem::refresh`,
        // pikiheadItem.cpp), its motion picked like `PikiHeadAI` does, with the leaf, bud or
        // flower on joint 3.
        let mut live = Vec::new();
        for sp in sim.sprouts() {
            live.push(sp.id);
            let track = self.sprouts.entry(sp.id).or_insert(SproutTrack {
                state: std::mem::discriminant(&sp.state),
                happa: sp.happa,
                motion: PikiHeadMotion::TaneFall,
            });
            let state = std::mem::discriminant(&sp.state);
            let entered = track.state != state;
            let grew = track.happa != sp.happa;
            track.motion = match sp.state {
                SproutState::Flying => PikiHeadMotion::TaneFall,
                SproutState::Buried { .. } => PikiHeadMotion::TaneDown,
                SproutState::Tane { .. } => PikiHeadMotion::TaneUp,
                SproutState::Ready { .. } if entered => PikiHeadMotion::Hatuga,
                SproutState::Ready { .. } if grew => PikiHeadMotion::UgGrow,
                SproutState::Ready { .. } => track.motion,
                SproutState::Wilted { .. } => PikiHeadMotion::Sioreru,
            };
            track.state = state;
            track.happa = sp.happa;
            let key = (4u64 << 40) | sp.id.0 as u64;
            // HATUGA and UGGROW play once, then UGWAIT loops.
            let mut motion = track.motion;
            if matches!(motion, PikiHeadMotion::Hatuga | PikiHeadMotion::UgGrow)
                && self.players.get(&key).is_some_and(|p| {
                    p.anim == motion as usize
                        && self
                            .pikihead_anims
                            .get(motion as usize)
                            .is_some_and(|(i, a)| {
                                p.frame >= (*i.keys.last().unwrap_or(&0) as f32).min(a.frames as f32 - 1.0)
                            })
                })
            {
                motion = PikiHeadMotion::UgWait;
                track.motion = motion;
            }
            let clock = match motion {
                PikiHeadMotion::TaneFall | PikiHeadMotion::UgWait => Clock::Loop(30.0),
                _ => Clock::Once(30.0),
            };
            let before = self.players.get(&key).copied();
            let anim = advance_index(&mut self.players, key, &self.pikihead_anims, motion as usize, clock, dt);
            // Sound keys crossed this frame (`AICreature::animationKeyUpdated` →
            // `PikiHeadItem::playSound`).
            if let (Some(after), Some((info, _))) =
                (self.players.get(&key).copied(), self.pikihead_anims.get(motion as usize))
            {
                let start = *info.keys.first().unwrap_or(&0) as f32;
                let end = *info.keys.last().unwrap_or(&0) as f32;
                let ranges: Vec<(f32, f32)> = match before {
                    Some(b) if b.anim == after.anim && after.frame >= b.frame => vec![(b.frame, after.frame)],
                    Some(b) if b.anim == after.anim => vec![(b.frame, end), (start, after.frame)],
                    _ => vec![(start, after.frame + 1e-3)],
                };
                for e in info.event_keys.iter().filter(|e| e.event == 0) {
                    let f = e.frame as f32;
                    if ranges.iter().any(|&(a, b)| f >= a && f < b) {
                        self.sprout_sounds.push((sp.onion, e.value, sp.pos));
                    }
                }
            }
            let root = if matches!(sp.state, SproutState::Flying) && sp.vel.length() > 0.0 {
                // Flying seeds point their Y axis against their velocity.
                let y = -sp.vel.normalize();
                let x = y.cross(GVec3::Z).normalize_or(GVec3::X);
                let z = x.cross(y).normalize();
                Mat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), sp.pos.extend(1.0))
            } else {
                srt_matrix(GVec3::ONE, GVec3::ZERO, sp.pos)
            };
            let joints = draw_model(
                &self.pikihead,
                root,
                anim,
                Look {
                    tint0: Some(piki_rgb(sp.color)),
                    ..Default::default()
                },
                &mut ctx,
            );
            if let Some(&j) = joints.get(3) {
                draw_model(&self.happa[sp.happa as usize], j, None, Look::default(), &mut ctx);
            }
        }
        self.sprouts.retain(|id, _| live.contains(id));

        // Nectar: falling drops ("leave", then "fall") and puddles, on the sim's own frames.
        for d in sim.nectar_drops() {
            let (motion, frame) = match d.state {
                libpikmin::nectar::DropState::Falling { frame, leaving: true } => (1, frame),
                libpikmin::nectar::DropState::Falling { frame, .. }
                | libpikmin::nectar::DropState::Splash { frame, .. } => (0, frame),
            };
            let anim = self.drop_anims.get(motion).map(|(_, a)| (a, frame.min(a.frames as f32 - 1.0)));
            let root = srt_matrix(GVec3::ONE, GVec3::ZERO, d.pos);
            draw_model(&self.drop, root, anim, Look::default(), &mut ctx);
        }
        for n in sim.puddles() {
            use libpikmin::PuddleState::*;
            let (motion, frame) = match n.state {
                Wait { frame } => (0, frame),
                Touched { frame } => (1, frame),
                Appear { frame } => (2, frame),
                Drain { frame } => (3, frame),
            };
            let anim = self.puddle_anims.get(motion).map(|(_, a)| (a, frame.min(a.frames as f32 - 1.0)));
            let root = srt_matrix(GVec3::ONE, GVec3::ZERO, n.pos);
            draw_model(&self.puddle, root, anim, Look::default(), &mut ctx);
        }

        // Enemies: their own models and `.key` animations at the disc scale. Corpses keep
        // their death pose wherever the sim carries the pellet.
        for t in &tekis.list {
            let (Some(spec), Some(gm)) = (tekis.spec(t.kind), self.tekis.get(t.kind.index()).and_then(Option::as_ref))
            else {
                continue;
            };
            let (motion, frame) = match t.ai {
                Ai::Corpse(_) => (spec.motions.dead, spec.keys(spec.motions.dead).end),
                _ => (t.motion, t.frame),
            };
            let anim = gm.1.get(motion).map(|(_, a)| (a, frame.clamp(0.0, a.frames as f32 - 1.0)));
            let root = srt_matrix(GVec3::splat(spec.scale), GVec3::new(0.0, t.yaw, 0.0), t.pos);
            draw_model(
                &gm.0,
                root,
                anim,
                Look {
                    brightness: (t.flash > 0.0).then_some(1.8),
                    ..Default::default()
                },
                &mut ctx,
            );
            if t.alive() && t.kind != TekiKind::Honeywisp {
                overlays.push(Overlay::Bar {
                    at: t.pos + GVec3::Y * (gm.0.hi.y * spec.scale + 10.0),
                    width: 50.0,
                    frac: (t.hp / spec.life).clamp(0.0, 1.0),
                    color: Color::from_rgba(80, 220, 80, 255),
                });
            }
        }

        // Pikmin.
        let pk = &params.piki;
        let piki_th = [
            pk.step_speed_threshold,
            pk.walk_speed_threshold,
            pk.run_speed_threshold,
            pk.escape_speed_threshold,
        ];
        let piki_rate = [
            pk.min_walk_anim_speed,
            pk.max_walk_anim_speed,
            pk.min_run_anim_speed,
            pk.max_run_anim_speed,
            pk.escape_anim_speed,
        ];
        for p in sim.pikis() {
            let mut scale = GVec3::ONE;
            let mut fade = 1.0;
            let speed = p.vel.with_y(0.0).length();
            let walk = locomotion(speed, piki_th, piki_rate);
            let (motion, clock) = match p.state {
                PikiState::Dead | PikiState::Swallowed { .. } => continue,
                PikiState::Idle | PikiState::Squad { .. } | PikiState::GoHang { .. } => walk,
                PikiState::Held { .. } => (Motion::Hang, Clock::Loop(30.0)),
                PikiState::Flying { .. } => (Motion::RollJmp, Clock::Loop(30.0)),
                PikiState::Attack(a) => match a.mode {
                    AttackMode::Approach => walk,
                    AttackMode::JumpPrep => (Motion::StillJump, Clock::Once(30.0)),
                    AttackMode::Jumping => (Motion::Jump, Clock::Once(30.0)),
                    AttackMode::Swing { frame, .. } => (Motion::Attack, Clock::Sim(frame)),
                    AttackMode::Latched { frame, .. } => (Motion::Kuttuku, Clock::Sim(frame)),
                },
                // `PikiFlickState`: JHIT flying, JKOKE landing (held while lying), GETUP.
                PikiState::Knocked { phase, .. } => match phase {
                    libpikmin::KnockPhase::Flying { frame } => (Motion::JHit, Clock::Sim(frame)),
                    libpikmin::KnockPhase::Landing { frame } => (Motion::JKoke, Clock::Sim(frame)),
                    libpikmin::KnockPhase::Down { .. } => (Motion::JKoke, Clock::Sim(params.anim.jkoke.end)),
                    libpikmin::KnockPhase::GettingUp { frame } => (Motion::GetUp, Clock::Sim(frame)),
                },
                PikiState::Carry(c) => match c.phase {
                    libpikmin::carry::CarryPhase::Lift { frame, .. } => {
                        (Motion::PickLoop, Clock::Sim(frame))
                    }
                    // `ActTransport::doLift`: PICK from frame 11, looping while it moves.
                    libpikmin::carry::CarryPhase::Move => (Motion::Pick, Clock::LoopFrom(30.0, 11.0)),
                    _ => walk,
                },
                PikiState::Pressed { .. } => {
                    scale = GVec3::new(1.8, 0.08, 1.8);
                    (Motion::Wait, Clock::Loop(0.0))
                }
                PikiState::Dying { frame, end } => {
                    // `PikiDyingState::init` picks DEAD, DEAD2 or DEAD3; their lengths tell
                    // which one the sim chose.
                    let m = if end < 109.0 {
                        Motion::Dead
                    } else if end < 121.0 {
                        Motion::Dead2
                    } else {
                        Motion::Dead3
                    };
                    (m, Clock::Sim(frame))
                }
                PikiState::Absorb { frame, .. } => (Motion::Mizunomi, Clock::Sim(frame)),
                // `ActEnter` / `ActExit`: climbing a leg, shrinking into the pod near the top.
                PikiState::Enter { climbing: false, .. } => walk,
                PikiState::Enter { ratio, .. } | PikiState::Exit { ratio, .. } => {
                    let k = libpikmin::onion_menu::SHRINK_FROM;
                    if ratio > k {
                        scale = GVec3::splat(((1.0 - ratio) / (1.0 - k)).clamp(0.0, 1.0));
                    }
                    (Motion::HNoboru, Clock::Loop(30.0))
                }
                PikiState::GrowUp { frame, second, .. } => (
                    if second { Motion::GrowUp2 } else { Motion::GrowUp1 },
                    Clock::Sim(frame),
                ),
                PikiState::Fading { timer } => {
                    fade = (timer / 1.5).clamp(0.0, 1.0);
                    scale = GVec3::splat(fade.max(0.05));
                    (Motion::Dead, Clock::Sim(1e6))
                }
            };
            let anim = advance(&mut self.players, p.id.0 as u64, &self.piki_anims, motion, clock, dt);
            let root = srt_matrix(scale, GVec3::new(0.0, p.face, 0.0), p.pos);
            let joints = draw_model(
                &self.pikmin[p.color.index()],
                root,
                anim,
                Look {
                    tint0: Some(piki_rgb(p.color)),
                    alpha: Some(fade),
                    ..Default::default()
                },
                &mut ctx,
            );
            // Leaf, bud or flower on joint 6 (`ViewPiki::demoDraw`).
            if let Some(&head) = joints.get(6) {
                draw_model(
                    &self.happa[p.happa as usize],
                    head,
                    None,
                    Look {
                        alpha: Some(fade),
                        ..Default::default()
                    },
                    &mut ctx,
                );
            }
        }

        // Captains.
        let nv = &params.navi;
        let navi_th = [
            nv.asibumi_start_speed,
            nv.walk_start_speed,
            nv.run_start_speed,
            nv.escape_start_speed,
        ];
        let navi_rate = [
            nv.walk_anim_min_frames,
            nv.walk_anim_max_frames,
            nv.run_anim_min_frames,
            nv.run_anim_max_frames,
            nv.escape_anim_min_frames,
        ];
        let damage_end = params.anim.damage.end;
        let ante = self.olimar.model.coll.iter().find(|c| c.id == "ante").cloned();
        let mut whistles = Vec::new();
        for (i, c) in sim.captains().iter().enumerate() {
            let speed = c.vel.with_y(0.0).length();
            let throwing = self.throw_timer.get(&c.id).is_some_and(|t| *t > 0.0);
            let (motion, clock) = if c.down {
                (Motion::ODead, Clock::Once(30.0))
            } else if c.damage_frames > 0.0 {
                (Motion::Damage, Clock::Sim(damage_end - c.damage_frames))
            } else if let Some(p) = c.punch {
                (Motion::Punch, Clock::Sim(p.frame))
            } else if let Some(libpikmin::onion::Pluck {
                phase: PluckPhase::Pull { frame, fast, .. },
                ..
            }) = c.pluck
            {
                (if fast { Motion::NukuFast } else { Motion::Nuku }, Clock::Sim(frame))
            } else if throwing {
                (Motion::Throw, Clock::Once(30.0))
            } else if matches!(c.throw, ThrowState::Wait { held: Some(_), .. }) {
                (Motion::ThrowWait, Clock::Loop(30.0))
            } else if !matches!(c.whistle, WhistleState::Off) && speed < nv.walk_start_speed {
                (Motion::Fue, Clock::Loop(30.0))
            } else {
                locomotion(speed, navi_th, navi_rate)
            };
            let key = (1u64 << 40) | c.id.0 as u64;
            let anim = advance(&mut self.players, key, &self.piki_anims, motion, clock, dt);
            let root = srt_matrix(GVec3::ONE, GVec3::new(0.0, c.face, 0.0), c.pos);
            let flash = c.damage_frames > 0.0 && (c.damage_frames as i32 / 2) % 2 == 0;
            let joints = draw_model(
                &self.olimar,
                root,
                anim,
                Look {
                    brightness: flash.then_some(1.6),
                    ..Default::default()
                },
                &mut ctx,
            );

            let col = if i == 0 {
                Color::from_rgba(250, 140, 40, 255)
            } else {
                Color::from_rgba(120, 200, 255, 255)
            };
            // The antenna's 'ante' sphere, where the whistle's beam starts (`Navi::demoDraw`).
            let antenna = ante
                .as_ref()
                .and_then(|a| Some(joints.get(a.joint)?.transform_point3(a.centre)))
                .unwrap_or(c.pos + GVec3::Y * 30.0);
            let wr = c.whistle_radius(&params);
            whistles.push(WhistleFrame {
                captain: c.id,
                state: c.whistle,
                antenna,
                cursor: c.cursor_world,
                captain_pos: c.pos,
                radius: wr,
            });
            if let Some((cursor, marker)) = self.cursor.as_ref() {
                // `Navi::refresh`: on the ground 1 above, Z pointing away from the captain.
                let at = c.cursor_world.with_y(c.cursor_world.y + 1.0);
                let z = (at - c.pos).with_y(0.0).normalize_or(GVec3::Z);
                let x = GVec3::Y.cross(z).normalize_or(GVec3::X);
                let root = Mat4::from_cols(x.extend(0.0), GVec3::Y.extend(0.0), z.extend(0.0), at.extend(1.0));
                draw_model(
                    cursor,
                    root,
                    None,
                    Look {
                        color_frame: Some(self.time * 30.0),
                        ..Default::default()
                    },
                    &mut ctx,
                );
                // The marker takes the next Pikmin to throw's colour (`findNextThrowPiki`:
                // the nearest squad Pikmin within 200), else white, unlit.
                let next = c.held_piki().map(|id| sim.piki(id).color).or_else(|| {
                    sim.pikis()
                        .iter()
                        .filter(|p| p.captain == Some(c.id) && matches!(p.state, PikiState::Squad { .. }))
                        .map(|p| (p, (p.pos - c.pos).length()))
                        .filter(|(_, d)| *d < 200.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(p, _)| p.color)
                });
                draw_model(
                    marker,
                    root,
                    None,
                    Look {
                        tint0: Some(next.map(piki_rgb).unwrap_or([255; 4])),
                        unlit: true,
                        ..Default::default()
                    },
                    &mut ctx,
                );
            } else {
                Self::ring(c.cursor_world, 8.0, col);
            }
            if wr > 0.0 && self.effects.is_none() {
                let a = if matches!(c.whistle, WhistleState::Calling { .. }) { 0.9 } else { 0.4 };
                Self::ring(c.cursor_world, wr, Color::new(1.0, 0.94, 0.47, a));
            }
            overlays.push(Overlay::Text {
                at: c.pos + GVec3::Y * 34.0,
                text: if i == 0 { "Olimar" } else { "Louie" }.into(),
                color: col,
                size: 18.0,
            });
            overlays.push(Overlay::Bar {
                at: c.pos + GVec3::Y * 28.0,
                width: 30.0,
                frac: (c.health / nv.health).clamp(0.0, 1.0),
                color: if c.health / nv.health > 0.25 {
                    Color::from_rgba(80, 220, 80, 255)
                } else {
                    Color::from_rgba(230, 60, 60, 255)
                },
            });
            if c.down {
                overlays.push(Overlay::Text {
                    at: c.pos + GVec3::Y * 44.0,
                    text: "DOWN (H)".into(),
                    color: Color::from_rgba(230, 60, 60, 255),
                    size: 18.0,
                });
            }
        }

        // Particles last: blended, without depth writes.
        let v = self.view_matrix();
        if let Some(fx) = self.effects.as_mut() {
            fx.update(&whistles, nv.whistle_hold_time, dt);
            let right = GVec3::new(v.x_axis.x, v.y_axis.x, v.z_axis.x);
            let up = GVec3::new(v.x_axis.y, v.y_axis.y, v.z_axis.y);
            fx.draw(right, up);
        }

        set_default_camera();
        overlays
    }

    fn draw_ground(&self, around: Vec3) {
        let half = 3000.0;
        let tile = 64.0; // world units per texture repeat
        let (cx, cz) = ((around.x / tile).round() * tile, (around.z / tile).round() * tile);
        let corner = |dx: f32, dz: f32| Vertex {
            position: vec3(cx + dx, 0.0, cz + dz),
            uv: vec2((cx + dx) / tile, (cz + dz) / tile),
            color: [255, 255, 255, 255],
            normal: vec4(0.0, 1.0, 0.0, 0.0),
        };
        draw_mesh(&Mesh {
            vertices: vec![
                corner(-half, -half),
                corner(half, -half),
                corner(half, half),
                corner(-half, half),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: Some(self.ground.clone()),
        });
        let c = Color::new(1.0, 1.0, 1.0, 0.07);
        let step = 100.0;
        let (gx, gz) = ((around.x / step).round() * step, (around.z / step).round() * step);
        for i in -15..=15 {
            let o = i as f32 * step;
            draw_line_3d(vec3(gx + o, 0.2, gz - 1500.0), vec3(gx + o, 0.2, gz + 1500.0), c);
            draw_line_3d(vec3(gx - 1500.0, 0.2, gz + o), vec3(gx + 1500.0, 0.2, gz + o), c);
        }
    }
}
