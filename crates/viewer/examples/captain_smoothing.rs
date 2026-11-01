//! Reproduce the RoR2 50 Hz host / 30 Hz sim / 60 Hz display mismatch.
//! cargo run --release -p viewer --example captain_smoothing -- <capture.png>
//! No replacement assets: models::OLIMAR (0), pikis/nv3Model.mod.
//! Walk spec: plugPikiKando/navi.cpp Navi::updateWalkAnimation / Navi::refresh.
use ::glam::{Vec2 as GVec2, Vec3 as GVec3};
use libpikmin::host::FlatHost;
use libpikmin::{CaptainInput, Params, Sim};
use libpikmin_assets::Assets;
use libpikmin_present::{view::kinds, View};
use macroquad::prelude::*;

fn config() -> Conf {
    Conf {
        window_title: "Olimar captain smoothing proof".into(),
        window_width: 1200,
        window_height: 740,
        ..Default::default()
    }
}

fn sample(points: &[(f64, GVec3)], time: f64) -> GVec3 {
    for pair in points.windows(2) {
        if time <= pair[1].0 {
            let a = ((time - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0., 1.) as f32;
            return pair[0].1.lerp(pair[1].1, a);
        }
    }
    points.last().unwrap().1
}

#[macroquad::main(config)]
async fn main() {
    let path = std::env::args().nth(1).expect("capture PNG path");
    let assets = Assets::discover(None).expect("extract your Pikmin disc first");
    let mut params = Params::default();
    params.load_p1_dir(&assets.root).unwrap();
    let mut sim = Sim::new(params, 9);
    let captain = sim.add_captain(GVec3::ZERO);
    sim.set_input(
        captain,
        CaptainInput {
            stick: GVec2::X,
            ..Default::default()
        },
    );
    let mut view = View::load(&assets).unwrap();
    let mut points = vec![(0., GVec3::ZERO)];
    let mut fixed = 0.;
    let mut textures = Vec::new();
    let mut panels = Vec::new();
    let mut errors = Vec::new();
    for frame in 1..=66 {
        let time = frame as f64 / 60.;
        while fixed + 0.02 <= time + 1e-9 {
            let count = sim.advance(&mut FlatHost, 0.02);
            fixed += 0.02;
            if count != 0 {
                points.push((sim.tick() as f64, sim.captain(captain).pos));
            }
        }
        let remainder = (time - fixed).clamp(0., 0.02) as f32;
        let shown = sim.tick() as f64 + sim.alpha() as f64 - (0.02 - remainder) as f64 * 30. - 1.;
        let body = sample(&points, shown);
        let alpha = (sim.alpha() + remainder * 30.).clamp(0., 1.);
        view.update(&sim, alpha, 1. / 60.);
        let draw = *view
            .draws
            .iter()
            .find(|d| d.kind == kinds::CAPTAIN)
            .unwrap();
        if frame > 12 {
            errors.push(draw.pos.x - body.x);
        }
        if frame >= 61 {
            panels.push((
                draw,
                body,
                view.positions.clone(),
                view.colors.clone(),
                view.texture_ids.clone(),
                view.uvs.clone(),
            ));
        }
    }
    let min = errors.iter().copied().fold(f32::INFINITY, f32::min);
    let max = errors.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    println!("Model 0 / pikis/nv3Model.mod: old root-camera offset {min:.3}..{max:.3}; shared feet offset 0");
    assert!(
        max - min > 0.1,
        "failed to reproduce the model-camera oscillation"
    );
    assert!(
        panels.windows(2).any(|p| p[0].2 != p[1].2),
        "walk must animate over several frames"
    );
    for t in &view.textures.list {
        let tex = Texture2D::from_rgba8(t.width as u16, t.height as u16, &t.rgba);
        tex.set_filter(FilterMode::Linear);
        textures.push(tex);
    }
    clear_background(Color::from_rgba(20, 28, 35, 255));
    for row in 0..2 {
        for (col, (draw, body, positions, colors, ids, uvs)) in panels.iter().enumerate() {
            let x = col as i32 * 200;
            let y = if row == 0 { 345 } else { 25 };
            let camera = Camera3D {
                position: vec3(0., 35., 100.),
                target: vec3(0., 18., 0.),
                up: Vec3::Y,
                fovy: 40f32.to_radians(),
                viewport: Some((x, y, 200, 295)),
                ..Default::default()
            };
            set_camera(&camera);
            draw_line_3d(vec3(-22., 0., 0.), vec3(22., 0., 0.), GREEN);
            draw_line_3d(vec3(0., 0., -10.), vec3(0., 0., 10.), GREEN);
            let at = if row == 0 {
                draw.pos - *body
            } else {
                GVec3::ZERO
            };
            let model = &view.models[draw.model as usize];
            let mut v = draw.first_vertex as usize;
            for (bi, batch) in model.batches.iter().enumerate() {
                let c = colors[draw.first_color as usize + bi];
                let vertices = (0..batch.vertices.len())
                    .map(|i| {
                        let p = at + draw.rot * positions[v + i];
                        let uv = uvs[v + i];
                        Vertex::new(
                            p.x,
                            p.y,
                            p.z,
                            uv[0],
                            uv[1],
                            Color::new(c[0], c[1], c[2], c[3]),
                        )
                    })
                    .collect();
                let tex = ids[draw.first_color as usize + bi];
                draw_mesh(&Mesh {
                    vertices,
                    indices: batch.indices.iter().map(|&i| i as u16).collect(),
                    texture: (tex >= 0).then(|| textures[tex as usize].clone()),
                });
                v += batch.vertices.len();
            }
        }
    }
    set_default_camera();
    draw_text(
        "Olimar model 0 | pikis/nv3Model.mod | six consecutive 60 Hz walk frames",
        16.,
        26.,
        24.,
        WHITE,
    );
    draw_text(
        "navi.cpp: Navi::updateWalkAnimation / Navi::refresh | host timing reproduction",
        16.,
        53.,
        21.,
        WHITE,
    );
    draw_text(
        &format!(
            "Before: separate view blend | root-camera oscillation {:.3} sim units",
            max - min
        ),
        16.,
        82.,
        22.,
        ORANGE,
    );
    draw_text(
        "After: model uses camera body's smoothed feet | root-camera offset 0",
        16.,
        407.,
        22.,
        GREEN,
    );
    get_screen_data().export_png(&path);
    next_frame().await;
}
