//! Viewer proof of the Dolphin's landing: the reverse takeoff motion, its touchdown dust and
//! the dust `View` spawns again when the landing ends. Captures every tick.
//!
//! cargo run --release -p viewer --example dolphin_land -- <out-dir> [ticks]
use ::glam::Vec3 as GVec3;
use libpikmin::host::FlatHost;
use libpikmin::{Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use macroquad::prelude::*;

fn config() -> Conf {
    Conf { window_title: "Dolphin landing".into(), window_width: 900, window_height: 900, ..Default::default() }
}
fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture2D {
    let t = Texture2D::from_rgba8(width as u16, height as u16, rgba);
    t.set_filter(FilterMode::Linear);
    t
}

#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "target/dolphin_land".into());
    let last: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(150);
    std::fs::create_dir_all(&out).expect("capture folder");

    let assets = Assets::discover(None).expect("extract your Pikmin disc first");
    let mut params = Params::default();
    params.load_p1_dir(&assets.root).ok();
    // The repaired Dolphin: an empty ship is the wreck, whose landing skips the authored
    // touchdown cloud and only gets View's fallback.
    libpikmin_present::view::set_repaired_ship(true);
    let mut view = View::load(&assets).unwrap();

    let mut sim = Sim::new(params, 7);
    let landing = GVec3::ZERO;
    let start = landing + GVec3::Y * 300.0;
    let ship = sim.add_ship(start, landing + GVec3::Z * 100.0, GVec3::Y * 74.0);
    for part in 0..30 {
        sim.give_ship_part(ship, part);
    }
    sim.set_onion_pose(ship, start, true);
    sim.set_onion_flight_motion(ship, true, 0.0);

    // `--close` frames the Dolphin like the in-game third-person camera, for judging how much
    // the landing dust covers the ship.
    let close = args.iter().any(|a| a == "--close");
    let eye = if close { vec3(150.0, 115.0, 200.0) } else { vec3(330.0, 210.0, 440.0) };
    let target = if close { vec3(0.0, 60.0, 0.0) } else { vec3(0.0, 65.0, 0.0) };
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward).normalize();
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

    // The host's descent: 3.5 s, eased, landing progress supplied while it is airborne.
    let descent = 3.5_f32;
    for tick in 0..=last {
        let t = (tick as f32 / 30.0 / descent).min(1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        let at = start.lerp(landing, eased);
        sim.set_onion_pose(ship, at, t < 1.0);
        if t < 1.0 {
            sim.set_onion_flight_motion(ship, true, t);
        }
        sim.step(&mut FlatHost);
        let events: Vec<_> = sim.drain_events().collect();
        for e in &events {
            view.on_event(e);
        }
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
        let quads = view.particles.quads.len();
        if tick % 10 == 0 || (85..=115).contains(&tick) {
            println!("tick {tick:3} t {t:.3} pos.y {:6.1} smoke quads {quads}", at.y);
        }
        let (mut big, mut extent, mut tex) = (0, 0.0f32, 0);
        for q in &view.particles.quads {
            let e = (q.corners[2] - q.corners[0]).length();
            if e > 120.0 { big += 1; }
            if e > extent { extent = e; tex = q.texture; }
        }
        let running = view.effects.as_ref().map_or(0, |e| e.running());
        if (80..=145).contains(&tick) {
            println!("tick {tick:3}: quads {quads} running {running} big {big} max extent {extent:.0} texture {tex}");
        }
        clear_background(Color::from_rgba(20, 28, 30, 255));
        set_camera(&Camera3D { position: eye, target, up: Vec3::Y, fovy: 42.0_f32.to_radians(), ..Default::default() });
        draw_plane(Vec3::ZERO, vec2(2000.0, 2000.0), None, Color::from_rgba(65, 85, 48, 255));
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
        draw_text("Dolphin landing", 18., 30., 26., WHITE);
        draw_text("reverse takeoff motion, touchdown dust (EFF_Rocket_Land / rkt_land.pcr)", 18., 58., 19., WHITE);
        draw_text(&format!("Tick {tick}   t {t:.3}   y {:.0}   smoke quads {quads}", at.y), 18., 86., 20., WHITE);
        draw_text(if tick < 97 { "descending" } else if tick < 105 { "touchdown dust" } else { "landing ends: dust again" }, 18., 114., 20., Color::from_rgba(220, 220, 160, 255));

        get_screen_data().export_png(&format!("{out}/tick_{tick:03}.png"));
        next_frame().await;
    }
    println!("{} captures in {out}", last + 1);
}
