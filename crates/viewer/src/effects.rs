//! The whistle's particle effects, run by `libpikmin_assets::particle` and drawn here.
//!
//! `NaviGatherState::init` starts two effects: `NaviWhistle` (three generators streaming from
//! the antenna to the cursor, NaviWhistle.h) and `NaviFue` (24 `g_curs00` sparkles riding the
//! whistle circle, placed by `Navi::renderCircle`). `NaviGatherState::exec` finishes them half
//! a second before the hold time runs out, or when the whistle ends. Generators step at the
//! game's 30 Hz (`particleManager::update` scales by 30 × frame time).

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Quat, Vec3};
use libpikmin::CaptainId;
use libpikmin::captain::WhistleState;
use libpikmin_assets::Assets;
use libpikmin_assets::particle::{EffectDef, Generator, Particle, Pcr, effects};
use macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, TextureWrap,
};
use macroquad::prelude::{
    FilterMode, Material, MaterialParams, Mesh, ShaderSource, Texture2D, UniformDesc, UniformType,
    Vertex, draw_mesh, gl_use_default_material, gl_use_material, load_material, vec2, vec3, vec4,
};

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec2 uv;
varying lowp vec4 prim;
varying lowp vec4 env;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    uv = texcoord;
    prim = color0 / 255.0;
    env = normal;
}"#;

/// `DGXGraphics::setBlendMode`'s TEV stage with C0 = primary, C1 = env (the decomp header's
/// `GX_CC_C2` is hardware C1), alpha `tex.a * prim.a`, and `drawPtclBillboard`'s alpha test
/// (≥ 1).
const FRAGMENT: &str = r#"#version 100
precision mediump float;
varying lowp vec2 uv;
varying lowp vec4 prim;
varying lowp vec4 env;
uniform sampler2D Texture;
uniform vec4 Mode;
void main() {
    vec4 t = texture2D(Texture, uv);
    vec3 c;
    float m = Mode.x;
    if (m < 0.5) { c = mix(prim.rgb, vec3(1.0), t.rgb); }
    else if (m < 1.5) { c = prim.rgb * t.rgb; }
    else if (m < 2.5) { c = mix(env.rgb, prim.rgb, t.rgb); }
    else if (m < 3.5) { c = env.rgb + t.rgb * prim.rgb; }
    else { c = prim.rgb; }
    float a = t.a * prim.a;
    if (a < 1.0 / 255.0) { discard; }
    gl_FragColor = vec4(clamp(c, 0.0, 1.0), a);
}"#;

/// GX blend factor `f` as a source (`src`) or destination factor.
fn factor(f: u8, src: bool) -> BlendFactor {
    match f {
        0 => BlendFactor::Zero,
        1 => BlendFactor::One,
        2 if src => BlendFactor::Value(BlendValue::DestinationColor),
        2 => BlendFactor::Value(BlendValue::SourceColor),
        3 if src => BlendFactor::OneMinusValue(BlendValue::DestinationColor),
        3 => BlendFactor::OneMinusValue(BlendValue::SourceColor),
        4 => BlendFactor::Value(BlendValue::SourceAlpha),
        5 => BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        6 => BlendFactor::Value(BlendValue::DestinationAlpha),
        _ => BlendFactor::OneMinusValue(BlendValue::DestinationAlpha),
    }
}

/// GX Z compare function.
fn compare(f: u8) -> Comparison {
    match f {
        0 => Comparison::Never,
        1 => Comparison::Less,
        2 => Comparison::Equal,
        3 => Comparison::LessOrEqual,
        4 => Comparison::Greater,
        5 => Comparison::NotEqual,
        6 => Comparison::GreaterOrEqual,
        _ => Comparison::Always,
    }
}

struct Def {
    pcr: Arc<Pcr>,
    tex: Texture2D,
    child_tex: Option<Texture2D>,
}

fn texture(assets: &Assets, path: &str) -> Option<Texture2D> {
    let (img, wrap_s, wrap_t) = libpikmin_assets::texture::decode_bti(&assets.read(path).ok()?).ok()?;
    let t = Texture2D::from_rgba8(img.width as u16, img.height as u16, &img.rgba);
    t.set_filter(FilterMode::Linear);
    let wrap = |w: bool| if w { TextureWrap::Repeat } else { TextureWrap::Clamp };
    // SAFETY: main thread, during loading.
    unsafe { macroquad::window::get_internal_gl() }
        .quad_context
        .texture_set_wrap(t.raw_miniquad_id(), wrap(wrap_s), wrap(wrap_t));
    Some(t)
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

/// One captain's whistle effects.
struct Whistle {
    /// `EFF_Navi_Whistle1..3` (A follows the antenna; B and C are placed along the beam).
    beam: Vec<Generator>,
    /// `NaviFue`: 24 sparkles on the circle.
    circle: Vec<Generator>,
    stopped: bool,
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
    beam: Vec<Def>,
    circle: Def,
    pipes: HashMap<(u8, u8), Material>,
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
        let beam = effects::NAVI_WHISTLE
            .iter()
            .map(|d| Def::load(assets, *d))
            .collect::<Option<Vec<_>>>()?;
        Some(Effects {
            beam,
            circle: Def::load(assets, effects::WHISTLE_CIRCLE)?,
            pipes: HashMap::new(),
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

    /// Starts, moves, steps and stops each captain's whistle effects.
    pub fn update(&mut self, frames: &[WhistleFrame], hold_time: f32, dt: f32) {
        for f in frames {
            let on = !matches!(f.state, WhistleState::Off);
            // `NaviGatherState::exec`: the effects stop 0.5 s before the call ends.
            let ending = matches!(f.state, WhistleState::Calling { timer, .. } if timer > hold_time - 0.5);
            if on && !self.whistles.contains_key(&f.captain) {
                let pcrs: Vec<_> = self.beam.iter().map(|d| d.pcr.clone()).collect();
                let mut beam: Vec<_> = pcrs.iter().map(|p| self.generator(p, f.antenna)).collect();
                // `setOrientedConstZAxis(true)` on B.
                beam[1].fixed_normal = true;
                let circle_pcr = self.circle.pcr.clone();
                let circle = (0..24).map(|_| self.generator(&circle_pcr, f.cursor)).collect();
                self.whistles.insert(f.captain, Whistle { beam, circle, stopped: false });
            }
            if let Some(w) = self.whistles.get_mut(&f.captain)
                && (!on || ending)
                && !w.stopped
            {
                w.stopped = true;
                w.beam.iter_mut().chain(w.circle.iter_mut()).for_each(Generator::finish);
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
                    g.emit_pos = f.cursor.with_y(0.0) + Vec3::new(f.radius * a.sin(), 0.0, f.radius * a.cos());
                    g.update(1.0, &mut |_| {});
                }
            }
            // Effects of captains that have left the list still run down.
            for (id, w) in self.whistles.iter_mut() {
                if frames.iter().any(|f| f.captain == *id) {
                    continue;
                }
                w.beam.iter_mut().chain(w.circle.iter_mut()).for_each(|g| {
                    g.finish();
                    g.update(1.0, &mut |_| {});
                });
            }
            self.whistles.retain(|_, w| {
                !(w.stopped
                    && w.beam.iter().chain(w.circle.iter()).all(|g| {
                        g.is_finished() && g.particles.is_empty() && g.children.is_empty()
                    }))
            });
        }
    }

    fn pipe(&mut self, blend: u8, z: u8) -> &Material {
        self.pipes.entry((blend, z)).or_insert_with(|| {
            load_material(
                ShaderSource::Glsl { vertex: VERTEX, fragment: FRAGMENT },
                MaterialParams {
                    pipeline_params: PipelineParams {
                        color_blend: Some(BlendState::new(
                            Equation::Add,
                            factor(blend & 0xF, true),
                            factor(blend >> 4, false),
                        )),
                        // `setBlendMode`'s Z mode: bit 3 test, bits 0-2 compare, bit 4 write.
                        depth_test: if z & 8 != 0 { compare(z & 7) } else { Comparison::Always },
                        depth_write: z & 0x10 != 0,
                        ..Default::default()
                    },
                    uniforms: vec![UniformDesc::new("Mode", UniformType::Float4)],
                    textures: vec![],
                },
            )
            .expect("particle shader compiles")
        })
    }

    /// Draws every live particle. Call with the 3D camera set; `right` and `up` are the
    /// camera's world axes.
    pub fn draw(&mut self, right: Vec3, up: Vec3) {
        type Loaded = (Arc<Pcr>, Texture2D, Option<Texture2D>);
        let mut batches: Vec<(Loaded, Vec<libpikmin_assets::particle::Quad>)> = Vec::new();
        let defs: Vec<Loaded> = self
            .beam
            .iter()
            .chain([&self.circle])
            .map(|d| (d.pcr.clone(), d.tex.clone(), d.child_tex.clone()))
            .collect();
        for w in self.whistles.values_mut() {
            for g in w.beam.iter_mut().chain(w.circle.iter_mut()) {
                let Some(def) = defs.iter().find(|d| Arc::ptr_eq(&d.0, &g.pcr)).cloned() else {
                    continue;
                };
                batches.push((def, g.quads(right, up)));
            }
        }
        for ((pcr, tex, child_tex), quads) in batches {
            let mode = pcr.color_mode() as f32;
            for child in [true, false] {
                // Children use the child texture and plain colour (`drawPtclChildren`).
                let tex = if child { child_tex.clone().unwrap_or_else(|| tex.clone()) } else { tex.clone() };
                let (blend, z) = (pcr.blend_factor, pcr.z_mode);
                let mut vertices = Vec::new();
                let mut indices = Vec::new();
                for q in quads.iter().filter(|q| q.child == child) {
                    let base = vertices.len() as u16;
                    let env = vec4(q.env[0] as f32, q.env[1] as f32, q.env[2] as f32, q.env[3] as f32) / 255.0;
                    for (k, c) in q.corners.iter().enumerate() {
                        let uv = [vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)][k];
                        vertices.push(Vertex {
                            position: vec3(c.x, c.y, c.z),
                            uv,
                            color: q.prim,
                            normal: env,
                        });
                    }
                    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
                }
                if vertices.is_empty() {
                    continue;
                }
                let mat = self.pipe(blend, z);
                // Children combine as `ZERO, ONE, C0, ZERO` = prim (mode 4).
                mat.set_uniform("Mode", vec4(if child { 4.0 } else { mode }, 0.0, 0.0, 0.0));
                gl_use_material(mat);
                draw_mesh(&Mesh {
                    vertices,
                    indices,
                    texture: Some(tex),
                });
                gl_use_default_material();
            }
        }
    }
}
