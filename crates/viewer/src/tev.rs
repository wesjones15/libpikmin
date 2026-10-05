//! A small GameCube TEV interpreter in GLSL, so PVW materials render the way the game set
//! them up: each stage's colour/alpha combiner, konst selections, the three colour registers,
//! and texcoord generators with their (animated) texture matrices.
//!
//! GX semantics follow `DGXGraphics::setMaterial` and `setMatMatrices` (sysDolphin/
//! dgxGraphics.cpp): stage inputs are `GXTevColorArg` / `GXTevAlphaArg`, the result is
//! `(d ± lerp(a, b, c) + bias) * scale`, written to PREV or a register. Texcoord n uses the
//! n-th generator; a generator of type `GX_TG_MTX2x4` takes the next texture data's matrix
//! unless its matrix is `GX_IDENTITY`. Lighting is done per vertex on the CPU and arrives as
//! the rasterized colour (channel 0 = diffuse, channel 1 = specular).

use libpikmin_assets::model::{TevCombiner, TevStage, mat_flags};
use libpikmin_assets::{Blend, Cull, Model};
use macroquad::math::{Mat4, Vec4, vec4};
use macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, CullFace, Equation, FrontFaceOrder,
    PipelineParams, TextureId, UniformDesc, UniformType,
};
use macroquad::prelude::{
    Material, MaterialParams, Mesh, ShaderSource, Texture2D, draw_mesh, gl_use_default_material,
    gl_use_material, load_material,
};

pub const MAX_STAGES: usize = 8;

const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying lowp vec2 uv;
varying lowp vec4 color;
varying highp vec4 nrm;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    nrm = normal;
}"#;

const FRAGMENT: &str = r#"#version 100
precision highp float;
varying lowp vec2 uv;
varying lowp vec4 color;
varying highp vec4 nrm;

uniform sampler2D Tex0;
uniform sampler2D Tex1;
uniform sampler2D Tex2;
uniform sampler2D Tex3;
uniform vec4 StageColor[8];
uniform vec4 StageColorOp[8];
uniform vec4 StageAlpha[8];
uniform vec4 StageAlphaOp[8];
uniform vec4 StageOrder[8];
uniform vec4 StageMisc[8];
uniform vec4 GenA[4];
uniform vec4 GenB[4];
uniform vec4 C0;
uniform vec4 C1;
uniform vec4 C2;
uniform vec4 K0;
uniform vec4 K1;
uniform vec4 K2;
uniform vec4 K3;
uniform vec4 Info;
uniform mat4 ViewRot;

vec4 konst(float sel, bool alpha) {
    if (sel < 7.5) { return vec4(1.0 - sel / 8.0); }
    vec4 k = K0;
    float idx = mod(sel - 12.0, 4.0);
    if (idx > 2.5) { k = K3; } else if (idx > 1.5) { k = K2; } else if (idx > 0.5) { k = K1; }
    if (sel < 15.5) { return k; }
    if (sel < 19.5) { return vec4(k.r); }
    if (sel < 23.5) { return vec4(k.g); }
    if (sel < 27.5) { return vec4(k.b); }
    return vec4(k.a);
}

vec3 cin(float a, vec4 p, vec4 r0, vec4 r1, vec4 r2, vec4 t, vec4 ras, vec4 k) {
    if (a < 0.5) { return p.rgb; }
    if (a < 1.5) { return vec3(p.a); }
    if (a < 2.5) { return r0.rgb; }
    if (a < 3.5) { return vec3(r0.a); }
    if (a < 4.5) { return r1.rgb; }
    if (a < 5.5) { return vec3(r1.a); }
    if (a < 6.5) { return r2.rgb; }
    if (a < 7.5) { return vec3(r2.a); }
    if (a < 8.5) { return t.rgb; }
    if (a < 9.5) { return vec3(t.a); }
    if (a < 10.5) { return ras.rgb; }
    if (a < 11.5) { return vec3(ras.a); }
    if (a < 12.5) { return vec3(1.0); }
    if (a < 13.5) { return vec3(0.5); }
    if (a < 14.5) { return k.rgb; }
    return vec3(0.0);
}

float ain(float a, vec4 p, vec4 r0, vec4 r1, vec4 r2, vec4 t, vec4 ras, vec4 k) {
    if (a < 0.5) { return p.a; }
    if (a < 1.5) { return r0.a; }
    if (a < 2.5) { return r1.a; }
    if (a < 3.5) { return r2.a; }
    if (a < 4.5) { return t.a; }
    if (a < 5.5) { return ras.a; }
    if (a < 6.5) { return k.a; }
    return 0.0;
}

float bias(float b) { return b < 0.5 ? 0.0 : (b < 1.5 ? 0.5 : -0.5); }
float scl(float s) { return s < 0.5 ? 1.0 : (s < 1.5 ? 2.0 : (s < 2.5 ? 4.0 : 0.5)); }

vec2 gen(vec4 a, vec4 b, vec2 tex, vec2 env) {
    vec2 src = (a.x > 0.5 && a.x < 1.5) ? env : tex;
    if (b.w < 0.5) { return src; }
    return vec2(a.y * src.x + a.z * src.y + a.w, b.x * src.x + b.y * src.y + b.z);
}

void main() {
    vec3 vn = normalize((ViewRot * vec4(nrm.xyz, 0.0)).xyz);
    vec2 env = vec2(vn.x, -vn.y);
    vec2 tc0 = gen(GenA[0], GenB[0], uv, env);
    vec2 tc1 = gen(GenA[1], GenB[1], uv, env);
    vec2 tc2 = gen(GenA[2], GenB[2], uv, env);
    vec2 tc3 = gen(GenA[3], GenB[3], uv, env);
    vec4 ras0 = color;
    // Channel 1 (specular): its material alpha is never set, so it stays GXInit white.
    vec4 ras1 = vec4(vec3(nrm.w), 1.0);

    vec4 prev = vec4(0.0);
    vec4 r0 = C0;
    vec4 r1 = C1;
    vec4 r2 = C2;
    for (int s = 0; s < 8; s++) {
        if (float(s) >= Info.x) { break; }
        vec4 order = StageOrder[s];
        vec2 tc = tc0;
        if (order.y > 2.5 && order.y < 3.5) { tc = tc3; }
        else if (order.y > 1.5 && order.y < 2.5) { tc = tc2; }
        else if (order.y > 0.5 && order.y < 1.5) { tc = tc1; }
        vec4 t = vec4(1.0);
        if (order.x < 0.5) { t = texture2D(Tex0, tc); }
        else if (order.x < 1.5) { t = texture2D(Tex1, tc); }
        else if (order.x < 2.5) { t = texture2D(Tex2, tc); }
        else if (order.x < 3.5) { t = texture2D(Tex3, tc); }
        vec4 ras = vec4(0.0);
        if (order.z > 3.5 && order.z < 4.5) { ras = ras0; }
        else if (order.z > 4.5 && order.z < 5.5) { ras = ras1; }
        else if (order.z < 0.5) { ras = ras0; }
        else if (order.z > 0.5 && order.z < 1.5) { ras = ras1; }
        vec4 misc = StageMisc[s];
        vec4 kc = konst(misc.y, false);
        vec4 ka = konst(misc.z, true);

        vec4 ca = StageColor[s];
        vec4 cop = StageColorOp[s];
        vec3 a = clamp(cin(ca.x, prev, r0, r1, r2, t, ras, kc), 0.0, 1.0);
        vec3 b = clamp(cin(ca.y, prev, r0, r1, r2, t, ras, kc), 0.0, 1.0);
        vec3 c = clamp(cin(ca.z, prev, r0, r1, r2, t, ras, kc), 0.0, 1.0);
        vec3 d = cin(ca.w, prev, r0, r1, r2, t, ras, kc);
        vec3 l = mix(a, b, c);
        vec3 rc = (d + (cop.x > 0.5 && cop.x < 1.5 ? -l : l) + bias(cop.y)) * scl(cop.z);
        rc = cop.w > 0.5 ? clamp(rc, 0.0, 1.0) : clamp(rc, -4.0, 4.0);

        vec4 aa = StageAlpha[s];
        vec4 aop = StageAlphaOp[s];
        float a1 = clamp(ain(aa.x, prev, r0, r1, r2, t, ras, ka), 0.0, 1.0);
        float b1 = clamp(ain(aa.y, prev, r0, r1, r2, t, ras, ka), 0.0, 1.0);
        float c1 = clamp(ain(aa.z, prev, r0, r1, r2, t, ras, ka), 0.0, 1.0);
        float d1 = ain(aa.w, prev, r0, r1, r2, t, ras, ka);
        float la = mix(a1, b1, c1);
        float ra = (d1 + (aop.x > 0.5 && aop.x < 1.5 ? -la : la) + bias(aop.y)) * scl(aop.z);
        ra = aop.w > 0.5 ? clamp(ra, 0.0, 1.0) : clamp(ra, -4.0, 4.0);

        if (order.w < 0.5) { prev.rgb = rc; } else if (order.w < 1.5) { r0.rgb = rc; }
        else if (order.w < 2.5) { r1.rgb = rc; } else { r2.rgb = rc; }
        if (misc.x < 0.5) { prev.a = ra; } else if (misc.x < 1.5) { r0.a = ra; }
        else if (misc.x < 2.5) { r1.a = ra; } else { r2.a = ra; }
    }
    vec4 outc = clamp(prev, 0.0, 1.0);
    if (Info.y > 0.5 && outc.a < 0.5) { discard; }
    gl_FragColor = vec4(outc.rgb * Info.z, outc.a * Info.w);
}"#;

fn uniforms() -> Vec<UniformDesc> {
    let mut u = Vec::new();
    for name in [
        "StageColor",
        "StageColorOp",
        "StageAlpha",
        "StageAlphaOp",
        "StageOrder",
        "StageMisc",
    ] {
        u.push(UniformDesc::new(name, UniformType::Float4).array(MAX_STAGES));
    }
    u.push(UniformDesc::new("GenA", UniformType::Float4).array(4));
    u.push(UniformDesc::new("GenB", UniformType::Float4).array(4));
    for name in ["C0", "C1", "C2", "K0", "K1", "K2", "K3", "Info"] {
        u.push(UniformDesc::new(name, UniformType::Float4));
    }
    u.push(UniformDesc::new("ViewRot", UniformType::Mat4));
    u
}

/// One material's static TEV program as uniform arrays.
#[derive(Clone)]
pub struct Program {
    pub stages: usize,
    color: [Vec4; MAX_STAGES],
    color_op: [Vec4; MAX_STAGES],
    alpha: [Vec4; MAX_STAGES],
    alpha_op: [Vec4; MAX_STAGES],
    order: [Vec4; MAX_STAGES],
    misc: [Vec4; MAX_STAGES],
}

fn args(c: &TevCombiner) -> (Vec4, Vec4) {
    (
        vec4(c.a as f32, c.b as f32, c.c as f32, c.d as f32),
        vec4(c.op as f32, c.bias as f32, c.scale as f32, c.clamp as u8 as f32),
    )
}

impl Program {
    pub fn new(stages: &[TevStage]) -> Self {
        let mut p = Program {
            stages: stages.len().min(MAX_STAGES),
            color: [Vec4::ZERO; MAX_STAGES],
            color_op: [Vec4::ZERO; MAX_STAGES],
            alpha: [Vec4::ZERO; MAX_STAGES],
            alpha_op: [Vec4::ZERO; MAX_STAGES],
            order: [Vec4::ZERO; MAX_STAGES],
            misc: [Vec4::ZERO; MAX_STAGES],
        };
        for (i, s) in stages.iter().take(MAX_STAGES).enumerate() {
            (p.color[i], p.color_op[i]) = args(&s.color);
            (p.alpha[i], p.alpha_op[i]) = args(&s.alpha);
            p.order[i] = vec4(
                s.tex_map as f32,
                s.tex_coord as f32,
                s.channel as f32,
                s.color.out as f32,
            );
            p.misc[i] = vec4(s.alpha.out as f32, s.kcolor_sel as f32, s.kalpha_sel as f32, 0.0);
        }
        p
    }

    /// Non-PVW materials: texture (if any) times the rasterized colour.
    pub fn modulate(textured: bool) -> Self {
        let comb = |a, b, c, d| TevCombiner {
            a,
            b,
            c,
            d,
            clamp: true,
            ..Default::default()
        };
        Self::new(&[TevStage {
            tex_coord: 0,
            tex_map: if textured { 0 } else { 0xFF },
            channel: 4,
            kcolor_sel: 12,
            kalpha_sel: 28,
            // ZERO + lerp(ZERO, TEXC, RASC) = TEXC * RASC
            color: comb(15, 8, 10, 15),
            alpha: comb(7, 4, 5, 7),
        }])
    }
}

/// Everything that varies per draw.
pub struct DrawParams {
    pub regs: [[f32; 4]; 3],
    pub konst: [[u8; 4]; 4],
    /// Per texcoord: (source, matrix rows if any).
    pub gens: [(u8, Option<[[f32; 3]; 2]>); 4],
    pub textures: [Option<Texture2D>; 4],
    pub blend: Blend,
    pub cull: Cull,
    /// Multiplies the output colour and alpha.
    pub brightness: f32,
    pub alpha: f32,
}

pub struct Tev {
    /// By cull mode, then opaque / alpha test / blended.
    pipes: [[Material; 3]; 3],
    bound: [[[Option<TextureId>; 4]; 3]; 3],
    white: Texture2D,
    pub view_rot: Mat4,
}

impl Tev {
    pub fn new() -> Self {
        let alpha = Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        ));
        let make = |blend: Option<BlendState>, depth_write: bool, cull: Cull| {
            load_material(
                ShaderSource::Glsl {
                    vertex: VERTEX,
                    fragment: FRAGMENT,
                },
                MaterialParams {
                    pipeline_params: PipelineParams {
                        depth_test: Comparison::LessOrEqual,
                        depth_write,
                        color_blend: blend,
                        // GX faces are front when clockwise on screen.
                        front_face_order: FrontFaceOrder::Clockwise,
                        cull_face: match cull {
                            Cull::Back => CullFace::Back,
                            Cull::Front => CullFace::Front,
                            Cull::None => CullFace::Nothing,
                        },
                        ..Default::default()
                    },
                    uniforms: uniforms(),
                    textures: vec!["Tex0".into(), "Tex1".into(), "Tex2".into(), "Tex3".into()],
                },
            )
            .expect("TEV shader compiles")
        };
        Tev {
            // opaque, alpha test, blended
            pipes: [Cull::Back, Cull::Front, Cull::None]
                .map(|c| [make(None, true, c), make(alpha, true, c), make(alpha, false, c)]),
            bound: [[[None; 4]; 3]; 3],
            white: Texture2D::from_rgba8(1, 1, &[255, 255, 255, 255]),
            view_rot: Mat4::IDENTITY,
        }
    }

    pub fn draw(&mut self, program: &Program, p: &DrawParams, mesh: Mesh) {
        let pi = match p.blend {
            Blend::Opaque => 0,
            Blend::AlphaTest => 1,
            Blend::AlphaBlend | Blend::Inverse => 2,
        };
        // Extra textures are per pipeline in macroquad, not per draw call: flush what was
        // batched before rebinding them.
        let want: [Option<TextureId>; 4] =
            std::array::from_fn(|i| p.textures[i].as_ref().map(|t| t.raw_miniquad_id()));
        let ci = p.cull as usize;
        if want != self.bound[ci][pi] {
            // SAFETY: main thread, between draws.
            unsafe { macroquad::window::get_internal_gl() }.flush();
            let mat = &self.pipes[ci][pi];
            for (i, name) in ["Tex0", "Tex1", "Tex2", "Tex3"].iter().enumerate() {
                mat.set_texture(name, p.textures[i].clone().unwrap_or_else(|| self.white.clone()));
            }
            self.bound[ci][pi] = want;
        }
        let mat = &self.pipes[ci][pi];
        mat.set_uniform_array("StageColor", &program.color[..]);
        mat.set_uniform_array("StageColorOp", &program.color_op[..]);
        mat.set_uniform_array("StageAlpha", &program.alpha[..]);
        mat.set_uniform_array("StageAlphaOp", &program.alpha_op[..]);
        mat.set_uniform_array("StageOrder", &program.order[..]);
        mat.set_uniform_array("StageMisc", &program.misc[..]);
        let mut gen_a = [Vec4::ZERO; 4];
        let mut gen_b = [Vec4::ZERO; 4];
        for (i, (src, m)) in p.gens.iter().enumerate() {
            match m {
                Some(m) => {
                    gen_a[i] = vec4(*src as f32, m[0][0], m[0][1], m[0][2]);
                    gen_b[i] = vec4(m[1][0], m[1][1], m[1][2], 1.0);
                }
                None => gen_a[i] = vec4(*src as f32, 0.0, 0.0, 0.0),
            }
        }
        mat.set_uniform_array("GenA", &gen_a[..]);
        mat.set_uniform_array("GenB", &gen_b[..]);
        for (i, r) in p.regs.iter().enumerate() {
            mat.set_uniform(["C0", "C1", "C2"][i], vec4(r[0], r[1], r[2], r[3]) / 255.0);
        }
        for (i, k) in p.konst.iter().enumerate() {
            let k = k.map(|v| v as f32 / 255.0);
            mat.set_uniform(["K0", "K1", "K2", "K3"][i], vec4(k[0], k[1], k[2], k[3]));
        }
        mat.set_uniform(
            "Info",
            vec4(
                program.stages as f32,
                (pi == 1) as u8 as f32,
                p.brightness,
                p.alpha,
            ),
        );
        mat.set_uniform("ViewRot", self.view_rot);
        gl_use_material(mat);
        draw_mesh(&mesh);
        gl_use_default_material();
    }
}

/// Which texture data's matrix each texcoord generator uses (`setMatMatrices`).
pub fn gen_params(model: &Model, mat: usize, time: f32) -> [(u8, Option<[[f32; 3]; 2]>); 4] {
    let m = &model.materials[mat];
    let mut out = [(4u8, None); 4];
    if m.flags & mat_flags::PVW == 0 {
        return out;
    }
    let mut j = 0;
    for (i, g) in m.tex_gens.iter().take(4).enumerate() {
        out[i].0 = g.src;
        if g.kind != 1 {
            continue;
        }
        if let Some(d) = m.tex_datas.get(j)
            && g.matrix != 10
        {
            // `PVWTextureData::animate`: the frame runs at 30 * speed per second.
            let frames = (d.frames.max(2) - 1) as f32;
            out[i].1 = d.matrix((time * 30.0 * d.speed).rem_euclid(frames));
        }
        j += 1;
    }
    out
}
