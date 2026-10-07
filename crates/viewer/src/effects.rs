//! Drawing `libpikmin_present::effects` (the whistle, the death ghost, the ship's effects) with
//! GL: each effect's quads with its texture in its GX blend and Z modes.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use glam::Vec3;
use libpikmin_assets::Assets;
use macroquad::miniquad::{
    BlendFactor, BlendState, BlendValue, Comparison, Equation, PipelineParams, TextureWrap,
};
use macroquad::prelude::{
    FilterMode, Material, MaterialParams, Mesh, ShaderSource, Texture2D, UniformDesc, UniformType,
    Vertex, draw_mesh, gl_use_default_material, gl_use_material, load_material, vec2, vec3, vec4,
};

pub use libpikmin_present::effects::WhistleFrame;

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


fn upload(t: &libpikmin_present::effects::Texture) -> Texture2D {
    let tex = Texture2D::from_rgba8(t.image.width as u16, t.image.height as u16, &t.image.rgba);
    tex.set_filter(FilterMode::Linear);
    let wrap = |w: bool| if w { TextureWrap::Repeat } else { TextureWrap::Clamp };
    // SAFETY: main thread, during loading.
    unsafe { macroquad::window::get_internal_gl() }
        .quad_context
        .texture_set_wrap(tex.raw_miniquad_id(), wrap(t.wrap[0]), wrap(t.wrap[1]));
    tex
}

/// `libpikmin_present`'s effects (which it derefs to), drawn with GL.
pub struct Effects {
    fx: libpikmin_present::effects::Effects,
    /// Per effect: its texture and its children's.
    textures: Vec<(Texture2D, Option<Texture2D>)>,
    pipes: HashMap<(u8, u8), Material>,
}

impl Deref for Effects {
    type Target = libpikmin_present::effects::Effects;
    fn deref(&self) -> &Self::Target {
        &self.fx
    }
}

impl DerefMut for Effects {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fx
    }
}

impl Effects {
    pub fn load(assets: &Assets) -> Option<Effects> {
        let fx = libpikmin_present::effects::Effects::load(assets)?;
        let textures = fx.defs().map(|d| (upload(&d.tex), d.child_tex.as_ref().map(upload))).collect();
        Some(Effects { fx, textures, pipes: HashMap::new() })
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
        for b in self.fx.batches(right, up) {
            let (tex, child_tex) = &self.textures[b.def];
            // Children use the child texture (`drawPtclChildren`).
            let tex = if b.child { child_tex.clone().unwrap_or_else(|| tex.clone()) } else { tex.clone() };
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for q in &b.quads {
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
            let mat = self.pipe(b.blend, b.z);
            mat.set_uniform("Mode", vec4(b.mode as f32, 0.0, 0.0, 0.0));
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
