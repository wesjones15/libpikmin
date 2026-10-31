//! Exact JPAC2-10 enemy emitters, including the projected indirect heat layer.
//! cargo run --release -p viewer --example p2_effects -- <capture.png> <tick> [body|gun|smoke|heat]
use ::glam::Vec3 as GVec3;
use libpikmin_assets::Assets;
use libpikmin_present::{effects::Flat, p2effects::P2Effects};
use macroquad::prelude::*;
fn config() -> Conf {
    Conf {
        window_title: "Pikmin 2 JPA2 source proof".into(),
        window_width: 1100,
        window_height: 760,
        ..Default::default()
    }
}
fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture2D {
    let t = Texture2D::from_rgba8(width as u16, height as u16, rgba);
    t.set_filter(FilterMode::Linear);
    t
}
#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let shot = args.get(1);
    let mode = args.get(3).map(String::as_str).unwrap_or("body");
    let last: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(26);
    let dir = Assets::default_dir().join("p2");
    let mut fx = P2Effects::load(&dir).unwrap();
    assert!(fx.missing.is_empty(), "missing {:?}", fx.missing);
    let mut flat = Flat::default();
    let eye = vec3(340., 250., 540.);
    let target = vec3(0., 70., 0.);
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward).normalize();
    let gv = |v: Vec3| GVec3::new(v.x, v.y, v.z);
    let mut effect_textures = Vec::new();
    let additive = load_material(ShaderSource::Glsl {
        vertex: "#version 100\nattribute vec3 position; attribute vec2 texcoord; attribute vec4 color0; uniform mat4 Model; uniform mat4 Projection; varying lowp vec4 c; varying mediump vec2 uv; void main(){gl_Position=Projection*Model*vec4(position,1.0); c=color0/255.0; uv=texcoord;}",
        fragment: "#version 100\nprecision lowp float; varying lowp vec4 c; varying mediump vec2 uv; uniform sampler2D Texture; void main(){gl_FragColor=texture2D(Texture,uv)*c;}",
    }, MaterialParams { pipeline_params: miniquad::PipelineParams {
        color_blend: Some(miniquad::BlendState::new(miniquad::Equation::Add, miniquad::BlendFactor::One, miniquad::BlendFactor::One)),
        depth_test: miniquad::Comparison::LessOrEqual, depth_write: false, ..Default::default()
    }, ..Default::default() }).unwrap();
    let scene = render_target(1100, 760);
    scene.texture.set_filter(FilterMode::Linear);
    let refraction=load_material(ShaderSource::Glsl {
        vertex:"#version 100\nattribute vec3 position; attribute vec2 texcoord; attribute vec4 color0; uniform mat4 Model; uniform mat4 Projection; varying lowp vec4 c; varying mediump vec2 uv; void main(){gl_Position=Projection*Model*vec4(position,1.0);c=color0/255.0;uv=texcoord;}",
        fragment:"#version 100\nprecision mediump float;varying lowp vec4 c;varying mediump vec2 uv;uniform sampler2D Texture;uniform sampler2D Scene;void main(){vec4 n=texture2D(Texture,uv);vec2 d=(vec2(n.r*n.a,n.g)*2.0-1.0)/4.0;vec2 at=gl_FragCoord.xy/vec2(1100.0,760.0);gl_FragColor=vec4(texture2D(Scene,at+d*0.035).rgb,n.b*c.a);}",
    },MaterialParams {textures:vec!["Scene".into()],pipeline_params:miniquad::PipelineParams {color_blend:Some(miniquad::BlendState::new(miniquad::Equation::Add,miniquad::BlendFactor::Value(miniquad::BlendValue::SourceAlpha),miniquad::BlendFactor::OneMinusValue(miniquad::BlendValue::SourceAlpha))),..Default::default()},..Default::default()}).unwrap();
    refraction.set_texture("Scene", scene.texture.clone());
    for tick in 0..=last {
        let groups: [&[u16]; 3] = match mode {
            "fur" => [&[0xf2], &[0xf3], &[0xf1, 0x25]],
            "feet" => [
                &[0x23, 0x28, 0x29, 0x2a],
                &[0x24, 0x25, 0x2b, 0x2c],
                &[0xf5, 0xf6],
            ],
            "heat" => [&[], &[0x1e9], &[]],
            "gun" => [&[0x9b], &[0x9d, 0x9e, 0x247], &[0x9f]],
            "smoke" => [&[0x27], &[0x8b], &[0x286]],
            _ => [
                &[0x2b4, 0x2b5],
                &[0x1e6, 0x1e7, 0x1e8, 0x1e9],
                &[0x283, 0x284, 0x285],
            ],
        };
        for (group, ids) in groups.iter().enumerate() {
            for (i, &id) in ids.iter().enumerate() {
                fx.request(
                    (group * 8 + i) as u32,
                    id,
                    ::glam::Mat4::from_translation(GVec3::new((group as f32 - 1.0) * 180., 0., 0.)),
                    1.0,
                );
            }
        }
        fx.advance(tick as u64);
        flat.quads.clear();
        fx.draw(&mut flat, gv(right), gv(up));
        while effect_textures.len() < flat.textures.len() {
            let t = &flat.textures[effect_textures.len()].image;
            effect_textures.push(texture(t.width, t.height, &t.rgba));
        }
        for (i, t) in flat.textures.iter().enumerate() {
            effect_textures[i].update(&Image {
                width: t.image.width as u16,
                height: t.image.height as u16,
                bytes: t.image.rgba.clone(),
            });
        }
        set_camera(&Camera3D {
            position: eye,
            target,
            up: Vec3::Y,
            fovy: 42.0_f32.to_radians(),
            render_target: Some(scene.clone()),
            ..Default::default()
        });
        clear_background(Color::from_rgba(20, 28, 30, 255));
        draw_plane(
            Vec3::ZERO,
            vec2(1100., 800.),
            None,
            Color::from_rgba(65, 85, 48, 255),
        );
        for z in -5..6 {
            for x in -5..6 {
                draw_cube(
                    vec3(x as f32 * 100., 70., z as f32 * 100. - 200.),
                    vec3(20., 140., 20.),
                    None,
                    if (x + z) % 2 == 0 {
                        LIGHTGRAY
                    } else {
                        DARKGRAY
                    },
                );
            }
        }
        for q in &flat.quads {
            if q.refraction {
                continue;
            }
            if q.additive {
                gl_use_material(&additive);
            } else {
                gl_use_default_material();
            }
            draw_mesh(&Mesh {
                vertices: q
                    .corners
                    .iter()
                    .zip(q.uvs)
                    .map(|(p, uv)| {
                        Vertex::new(
                            p.x,
                            p.y,
                            p.z,
                            uv[0],
                            uv[1],
                            Color::new(q.color[0], q.color[1], q.color[2], q.color[3]),
                        )
                    })
                    .collect(),
                indices: vec![0, 1, 2, 0, 2, 3],
                texture: Some(effect_textures[q.texture as usize].clone()),
            });
        }
        gl_use_default_material();
        set_default_camera();
        draw_texture_ex(
            &scene.texture,
            0.,
            0.,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(1100., 760.)),
                flip_y: true,
                ..Default::default()
            },
        );
        set_camera(&Camera3D {
            position: eye,
            target,
            up: Vec3::Y,
            fovy: 42.0_f32.to_radians(),
            ..Default::default()
        });
        gl_use_material(&refraction);
        for q in flat.quads.iter().filter(|q| q.refraction) {
            draw_mesh(&Mesh {
                vertices: q
                    .corners
                    .iter()
                    .zip(q.uvs)
                    .map(|(p, uv)| {
                        Vertex::new(
                            p.x,
                            p.y,
                            p.z,
                            uv[0],
                            uv[1],
                            Color::new(1., 1., 1., q.color[3]),
                        )
                    })
                    .collect(),
                indices: vec![0, 1, 2, 0, 2, 3],
                texture: Some(effect_textures[q.texture as usize].clone()),
            });
        }
        gl_use_default_material();
        set_default_camera();
        draw_text(
            match mode {
                "fur" => "BigFoot::setupEffect/startFurEffect | fur 0xF2/F3 | lift dust 0xF1/25",
                "feet" => {
                    "TDamaFoot/Walk | water 0x23,28-2A | ground 0x24,25,2B,2C | TOdamaWalk F5/F6"
                }
                "heat" => "Fiery Bulblax | indirect heat emitter 0x1E9 | efx::TYakiBody",
                "gun" => "JPA2 Man-at-Legs | shell 0x9B | shot 0x9D/9E/247 | sight 0x9F",
                "smoke" => "JPA2 | pinch smoke 0x27 | sleep bubble 0x8B | steam 0x286",
                _ => "JPA2 | ground gas 0x2B4/2B5 | flames 0x1E6-1E9 | hair 0x283-285",
            },
            18.,
            27.,
            23.,
            WHITE,
        );
        draw_text(
            "ParticleID.h + efx/THdama.h, TGasuHiba.h, TYaki.h, TKechappy.h | shared JParticle runtime",
            18.,
            54.,
            21.,
            WHITE,
        );
        draw_text(
            &format!(
                "Frame {tick} | {} quads | TEV alpha + scene-refraction pass",
                flat.quads.len()
            ),
            18.,
            81.,
            21.,
            WHITE,
        );
        if tick == last {
            if let Some(path) = shot {
                get_screen_data().export_png(path);
            }
            break;
        }
        next_frame().await;
    }
}
