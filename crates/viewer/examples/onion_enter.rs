//! Viewer proof of a squad returning to a flying Onion: walk to the leg the sim targets, the
//! host's jump to its midpoint, then the climb (`ActEnter`, aiEnter.cpp; the jump is the host
//! extension in `onion_menu::ENTER_JUMP_SECONDS`).
//!
//! The disc models are right-handed. Unity draws each model with its local X mirrored, and
//! `View` mirrors the Onion's leg points to match. Passing `--unity` renders the way the host
//! does (mirrored model vertices + mirrored leg points + `sim.set_mirror_onion_legs`) so the
//! two can be compared frame for frame; the default renders the right-handed viewer's own
//! frame.
//!
//! cargo run --release -p viewer --example onion_enter -- <out-dir> [frames] [--unity]
use ::glam::Vec3 as GVec3;
use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Params, PikiColor, Sim};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use macroquad::prelude::*;

fn config() -> Conf {
    Conf { window_title: "Pikmin return to a flying Onion".into(), window_width: 900, window_height: 900, ..Default::default() }
}
fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture2D {
    let t = Texture2D::from_rgba8(width as u16, height as u16, rgba);
    t.set_filter(FilterMode::Linear);
    t
}

#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "target/onion_enter".into());
    let last: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(220);
    let unity = args.iter().any(|a| a == "--unity");
    std::fs::create_dir_all(&out).expect("capture folder");

    let assets = Assets::discover(None).expect("extract your Pikmin disc first");
    let mut params = Params::default();
    params.load_p1_dir(&assets.root).ok();
    let mut view = View::load(&assets).unwrap();
    // The viewer draws the disc models unmirrored; only the Unity host flips each model's
    // local X (ror2-common ModelView.MirrorX).
    view.set_model_mirror_x(unity);

    let mut sim = Sim::new(params, 44);
    // The same switch the plugin sets: the Onion's drawn model (and so its legs) is mirrored.
    sim.set_mirror_onion_legs(unity);
    let onion = sim.add_onion(GVec3::new(0.0, 175.0, 0.0), &[PikiColor::Red, PikiColor::Yellow, PikiColor::Blue]);
    sim.set_onion_pose(onion, GVec3::new(0.0, 175.0, 0.0), true);
    let captain = sim.add_captain(GVec3::new(0.0, 0.0, 210.0));
    // One of each colour, so the three legs are all used; they start on the ground in front.
    let squad = [
        sim.spawn_piki_in_squad(PikiColor::Red, GVec3::new(-70.0, 0.0, 150.0), captain).unwrap(),
        sim.spawn_piki_in_squad(PikiColor::Yellow, GVec3::new(0.0, 0.0, 165.0), captain).unwrap(),
        sim.spawn_piki_in_squad(PikiColor::Blue, GVec3::new(70.0, 0.0, 150.0), captain).unwrap(),
    ];
    let sent = sim.onion_put_away(captain, onion, PikiColor::Red, 1)
        + sim.onion_put_away(captain, onion, PikiColor::Yellow, 1)
        + sim.onion_put_away(captain, onion, PikiColor::Blue, 1);
    println!("sent {sent} Pikmin back to the Onion");

    let eye = vec3(330.0, 300.0, 430.0);
    let target = vec3(0.0, 180.0, 0.0);
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

    let mut shot = false;
    for tick in 0..=last {
        sim.set_input(captain, CaptainInput::default());
        sim.step(&mut FlatHost);
        let events: Vec<_> = sim.drain_events().collect();
        for e in &events {
            println!("tick {tick}: {e:?}");
            view.on_event(e);
        }
        view.set_camera(gv(right), gv(up), gv(forward));
        view.update(&sim, 1.0, 1.0 / 30.0);
        view.particles(gv(right), gv(up));

        let states: Vec<_> = squad.iter().map(|id| {
            let p = sim.piki(*id);
            match p.state {
                libpikmin::PikiState::Enter { climbing: false, ratio, .. } if ratio < 0.0 =>
                    format!("{} jump {:.2}s left {:?}", colour(p.color), -ratio, p.pos),
                libpikmin::PikiState::Enter { climbing: true, ratio, .. } =>
                    format!("{} climb {:.2}", colour(p.color), ratio),
                libpikmin::PikiState::Enter { climbing: false, .. } => format!("{} walk {:?}", colour(p.color), p.pos),
                libpikmin::PikiState::Dead => format!("{} stored", colour(p.color)),
                ref other => format!("{} {other:?}", colour(p.color)),
            }
        }).collect();
        // Drawn PIKI draws are in sim order: the same order as `sim.pikis()`.
        let piki_draws: Vec<GVec3> = view.draws.iter()
            .filter(|d| d.kind == libpikmin_present::view::kinds::PIKI).map(|d| d.pos).collect();
        for (i, id) in squad.iter().enumerate() {
            let p = sim.piki(*id);
            if let libpikmin::PikiState::Enter { climbing: false, ratio, .. } = p.state
                && ratio < 0.0
                && let Some(drawn) = piki_draws.get(i)
            {
                println!("tick {tick:3} {} jump: sim {:?} drawn {:?} delta {:.1}",
                    colour(p.color), p.pos, drawn, (*drawn - p.pos).length());
            }
            if let libpikmin::PikiState::Enter { climbing: true, ratio, .. } = p.state
                && let Some(drawn) = piki_draws.get(i)
                && (ratio - 0.5).abs() < 0.02
            {
                println!("tick {tick:3} {} climb {:.2}: sim {:?} drawn {:?} delta {:.1}",
                    colour(p.color), ratio, p.pos, drawn, (*drawn - p.pos).length());
            }
        }
        if tick % 10 == 0 { println!("tick {tick}: {}", states.join(" | ")); }

        while textures.len() < view.textures.list.len() {
            let t = &view.textures.list[textures.len()];
            textures.push(texture(t.width, t.height, &t.rgba));
        }
        while effect_textures.len() < view.particles.textures.len() {
            let t = &view.particles.textures[effect_textures.len()].image;
            effect_textures.push(texture(t.width, t.height, &t.rgba));
        }
        clear_background(Color::from_rgba(20, 28, 30, 255));
        set_camera(&Camera3D { position: eye, target, up: Vec3::Y, fovy: 42.0_f32.to_radians(), ..Default::default() });
        draw_plane(Vec3::ZERO, vec2(900.0, 900.0), None, Color::from_rgba(65, 85, 48, 255));
        for draw in &view.draws {
            let model = &view.models[draw.model as usize];
            let mut vertex = draw.first_vertex as usize;
            for (bi, batch) in model.batches.iter().enumerate() {
                let c = view.colors[draw.first_color as usize + bi];
                let color = Color::new(c[0], c[1], c[2], c[3] * draw.alpha);
                let vertices = (0..batch.vertices.len()).map(|i| {
                    let local = view.positions[vertex + i];
                    // Unity: each model's local X is mirrored (ModelView.MirrorX).
                    let local = if unity { GVec3::new(-local.x, local.y, local.z) } else { local };
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
        draw_text("Pikmin return to a flying Onion", 18., 30., 26., WHITE);
        draw_text(if unity {
            "Unity host frame: models mirrored, leg points compensated (View model_mirror_x = true)"
        } else {
            "Right-handed reference: disc models unmirrored (View model_mirror_x = false)"
        }, 18., 58., 20., WHITE);
        draw_text("aiEnter.cpp ActEnter: walk to the leg, jump to its midpoint (host extension), climb HNoboru", 18., 84., 19., WHITE);
        draw_text(&format!("Tick {tick}   {}", states.join("  |  ")), 18., 112., 19., WHITE);
        if let Some(top) = view.draws.iter().find(|d| d.kind == libpikmin_present::view::kinds::ONION) {
            draw_text(&format!("Onion drawn at {:?}", top.pos), 18., 140., 18., Color::from_rgba(180, 200, 180, 255));
        }

        let path = format!("{out}/tick_{tick:03}.png");
        get_screen_data().export_png(&path);
        shot = true;
        next_frame().await;
    }
    println!("{} captures in {out} (unity = {unity}, shot = {shot})", last + 1);
}

fn colour(c: PikiColor) -> &'static str {
    match c {
        PikiColor::Blue => "Blue",
        PikiColor::Red => "Red",
        PikiColor::Yellow => "Yellow",
        PikiColor::Purple => "Purple",
        PikiColor::White => "White",
    }
}
