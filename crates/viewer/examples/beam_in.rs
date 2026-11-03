//! Where the Dolphin's beam-in puts Olimar: the ship's goal spot (its glowing rings) against
//! the captain's held arrival pose. The host places the ship 3 m behind him, and the ship's
//! goal is 100 sim units (4 m) ahead of the ship, leaving the ring 1 m ahead of him.
//!
//! cargo run --release -p viewer --example beam_in -- <out.png> [gap-units]
use ::glam::Vec3 as GVec3;
use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use macroquad::prelude::*;

fn config() -> Conf {
    Conf { window_title: "Dolphin beam-in".into(), window_width: 900, window_height: 900, ..Default::default() }
}
fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture2D {
    let t = Texture2D::from_rgba8(width as u16, height as u16, rgba);
    t.set_filter(FilterMode::Linear);
    t
}

#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let shot = args.get(1).cloned().unwrap_or_else(|| "target/beam_in.png".into());
    // The captain's distance ahead of the ship, in sim units: 75 = 3 m at Scale 0.04.
    let gap: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(75.0);
    let assets = Assets::discover(None).expect("extract your Pikmin disc first");
    let mut params = Params::default();
    params.load_p1_dir(&assets.root).ok();
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(params, 7);
    let ship = sim.add_ship(GVec3::ZERO, GVec3::new(0.0, 0.0, 100.0), GVec3::Y * 74.0);
    let captain = sim.add_captain(GVec3::new(0.0, 0.0, gap));
    sim.set_input(captain, CaptainInput::default());
    view.set_materialize(captain.0, 0.99);

    let eye = vec3(0.0, 600.0, 55.0);
    let target = vec3(0.0, 0.0, 55.0);
    let forward = (target - eye).normalize();
    let right = vec3(1.0, 0.0, 0.0);
    let up = vec3(0.0, 0.0, 1.0);
    let gv = |v: Vec3| GVec3::new(v.x, v.y, v.z);

    let mut textures = Vec::new();
    let mut effect_textures = Vec::new();
    let additive = load_material(ShaderSource::Glsl {
        vertex: "#version 100\nattribute vec3 position; attribute vec2 texcoord; attribute vec4 color0; uniform mat4 Model; uniform mat4 Projection; varying lowp vec4 c; varying mediump vec2 uv; void main(){gl_Position=Projection*Model*vec4(position,1.0); c=color0/255.0; uv=texcoord;}",
        fragment: "#version 100\nprecision lowp float; varying lowp vec4 c; varying mediump vec2 uv; uniform sampler2D Texture; void main(){gl_FragColor=texture2D(Texture,uv)*c;}",
    }, MaterialParams { pipeline_params: miniquad::PipelineParams {
        color_blend: Some(miniquad::BlendState::new(miniquad::Equation::Add, miniquad::BlendFactor::One, miniquad::BlendFactor::One)),
        depth_test: miniquad::Comparison::LessOrEqual, depth_write: false, ..Default::default()
    }, ..Default::default() }).unwrap();

    for tick in 0..30 {
        sim.step(&mut FlatHost);
        view.set_materialize(captain.0, 0.99);
        view.set_camera(gv(right), gv(up), gv(forward));
        view.update(&sim, 1.0, 1.0 / 30.0);
        view.particles(gv(right), gv(up));
        while textures.len() < view.textures.list.len() {
            let t = &view.textures.list[textures.len()];
            textures.push(texture(t.width, t.height, &t.rgba));
        }
        while effect_textures.len() < view.particles.textures.len() {
            let t = &view.particles.textures[effect_textures.len()].image;
            effect_textures.push(texture(t.width, t.height, &t.rgba));
        }
        if tick == 29 {
            println!("captain sim {:?}, ship goal {:?}", sim.captain(captain).pos, sim.onions()[ship.0 as usize].goal_pos());
            let piki = sim.captains().len();
            println!("captains {piki}");
        }
        clear_background(Color::from_rgba(20, 28, 30, 255));
        set_camera(&Camera3D { position: eye, target, up, fovy: 42.0_f32.to_radians(), ..Default::default() });
        draw_plane(Vec3::ZERO, vec2(600.0, 600.0), None, Color::from_rgba(65, 85, 48, 255));
        let cross = |p: Vec3, c: Color| {
            draw_line_3d(p - vec3(12.0, 0.0, 0.0), p + vec3(12.0, 0.0, 0.0), c);
            draw_line_3d(p - vec3(0.0, 0.0, 12.0), p + vec3(0.0, 0.0, 12.0), c);
        };
        cross(vec3(0.0, 2.0, 100.0), WHITE);
        cross(vec3(0.0, 2.0, gap), Color::from_rgba(255, 60, 60, 255));
        for draw in &view.draws {
            let model = &view.models[draw.model as usize];
            let mut vertex = draw.first_vertex as usize;
            for (bi, batch) in model.batches.iter().enumerate() {
                let c = view.colors[draw.first_color as usize + bi];
                let color = Color::new(c[0], c[1], c[2], c[3] * draw.alpha);
                let vertices = (0..batch.vertices.len()).map(|i| {
                    let local = view.positions[vertex + i];
                    let p = draw.pos + draw.rot * local;
                    let uv = view.uvs[vertex + i];
                    Vertex::new(p.x, p.y, p.z, uv[0], uv[1], color)
                }).collect();
                let tex = view.texture_ids[draw.first_color as usize + bi];
                draw_mesh(&Mesh { vertices, indices: batch.indices.iter().map(|&i| i as u16).collect(), texture: (tex >= 0).then(|| textures[tex as usize].clone()) });
                vertex += batch.vertices.len();
            }
        }
        for quad in &view.particles.quads {
            if quad.additive { gl_use_material(&additive); } else { gl_use_default_material(); }
            let c = quad.color;
            let vertices = quad.corners.iter().zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]]).map(|(p, uv)| Vertex::new(p.x, p.y, p.z, uv[0], uv[1], Color::new(c[0], c[1], c[2], c[3]))).collect();
            draw_mesh(&Mesh { vertices, indices: vec![0, 1, 2, 0, 2, 3], texture: Some(effect_textures[quad.texture as usize].clone()) });
        }
        gl_use_default_material();
        set_default_camera();
        draw_text(&format!("Dolphin goal (ring) at 100 units; Olimar held at {gap:.0} units"), 18., 30., 22., WHITE);
        draw_text(if (gap - 100.0).abs() < 1.0 { "centred" } else { "off-centre" }, 18., 58., 22., WHITE);
        if tick == 29 {
            if !shot.is_empty() {
                get_screen_data().export_png(&shot);
            }
            break;
        }
        next_frame().await;
    }
}
