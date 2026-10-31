//! Source-identified pluck clips and dirt, through the same View used by the RoR2 host.
//! cargo run --release -p viewer --example pluck -- <capture.png> <tick>
use ::glam::Vec3 as GVec3;
use libpikmin::{Params, Sim, PikiColor, CaptainInput, PikiState};
use libpikmin::host::FlatHost;
use libpikmin_assets::Assets;
use libpikmin_present::View;
use macroquad::prelude::*;
fn config() -> Conf { Conf { window_title: "Pikmin pluck source proof".into(), window_width: 1100, window_height: 760, ..Default::default() } }
fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture2D {
    let t = Texture2D::from_rgba8(width as u16,height as u16,rgba); t.set_filter(FilterMode::Linear); t
}
#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let shot = args.get(1);
    let last: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(26);
    let assets = Assets::discover(None).expect("extract your Pikmin disc first");
    let mut params=Params::default(); params.load_p1_dir(&assets.root).unwrap();
    println!("pluck timings: {:?} {:?}", params.anim.nukareru, params.anim.nukare_fast);
    let mut view=View::load(&assets).unwrap();
    let mut sim=Sim::new(params,9);
    let captain=sim.add_captain(GVec3::ZERO);
    sim.plant_sprout(PikiColor::Red,GVec3::new(0.,0.,6.));
    sim.plant_sprout(PikiColor::Yellow,GVec3::new(36.,0.,6.));
    let eye=vec3(95.,65.,120.); let target=vec3(12.,8.,0.);
    let forward=(target-eye).normalize(); let right=forward.cross(Vec3::Y).normalize(); let up=right.cross(forward).normalize();
    let gv=|v:Vec3|GVec3::new(v.x,v.y,v.z);
    let mut textures=Vec::new(); let mut effect_textures=Vec::new();
    let additive = load_material(ShaderSource::Glsl {
        vertex: "#version 100\nattribute vec3 position; attribute vec2 texcoord; attribute vec4 color0; uniform mat4 Model; uniform mat4 Projection; varying lowp vec4 c; varying mediump vec2 uv; void main(){gl_Position=Projection*Model*vec4(position,1.0); c=color0/255.0; uv=texcoord;}",
        fragment: "#version 100\nprecision lowp float; varying lowp vec4 c; varying mediump vec2 uv; uniform sampler2D Texture; void main(){gl_FragColor=texture2D(Texture,uv)*c;}",
    }, MaterialParams { pipeline_params: miniquad::PipelineParams {
        color_blend: Some(miniquad::BlendState::new(miniquad::Equation::Add, miniquad::BlendFactor::One, miniquad::BlendFactor::One)),
        depth_test: miniquad::Comparison::LessOrEqual, depth_write: false, ..Default::default()
    }, ..Default::default() }).unwrap();
    for tick in 0..=last {
        sim.set_input(captain,CaptainInput { throw:true,..Default::default() });
        sim.step(&mut FlatHost);
        let events:Vec<_>=sim.drain_events().collect();
        for e in &events { println!("tick {tick}: {e:?}"); view.on_event(e); }
        let status=sim.pikis().iter().filter_map(|p| if let PikiState::Plucked{frame,fast,..}=p.state {Some(format!("{:?} {} frame {frame:.0}",p.color,if fast {"fast"}else{"normal"}))}else{None}).collect::<Vec<_>>().join(" | ");
        view.set_camera(gv(right),gv(up),gv(forward));
        view.update(&sim,1.0,1.0/30.0); view.particles(gv(right),gv(up));
        while textures.len() < view.textures.list.len() { let t=&view.textures.list[textures.len()]; textures.push(texture(t.width,t.height,&t.rgba)); }
        while effect_textures.len() < view.particles.textures.len() { let t=&view.particles.textures[effect_textures.len()].image; effect_textures.push(texture(t.width,t.height,&t.rgba)); }
        clear_background(Color::from_rgba(20, 28, 30, 255));
        set_camera(&Camera3D { position: eye, target, up: Vec3::Y, fovy: 42.0_f32.to_radians(), ..Default::default() });
        draw_plane(Vec3::ZERO, vec2(600.0, 500.0), None, Color::from_rgba(65, 85, 48, 255));
        for draw in &view.draws {
            let model = &view.models[draw.model as usize];
            let mut vertex = draw.first_vertex as usize;
            for (bi, batch) in model.batches.iter().enumerate() {
                let c = view.colors[draw.first_color as usize + bi];
                let color = Color::new(c[0], c[1], c[2], c[3] * draw.alpha);
                let vertices = (0..batch.vertices.len()).map(|i| {
                    let p = draw.pos + draw.rot * view.positions[vertex+i];
                    let uv = view.uvs[vertex+i];
                    Vertex::new(p.x,p.y,p.z,uv[0],uv[1],color)
                }).collect();
                let tex = view.texture_ids[draw.first_color as usize + bi];
                draw_mesh(&Mesh { vertices, indices: batch.indices.iter().map(|&i| i as u16).collect(), texture: (tex>=0).then(|| textures[tex as usize].clone()) });
                vertex += batch.vertices.len();
            }
        }
        for quad in &view.particles.quads {
            if quad.additive { gl_use_material(&additive); } else { gl_use_default_material(); }
            let c = quad.color;
            let vertices = quad.corners.iter().zip([[0.,0.],[1.,0.],[1.,1.],[0.,1.]]).map(|(p,uv)| Vertex::new(p.x,p.y,p.z,uv[0],uv[1],Color::new(c[0],c[1],c[2],c[3]))).collect();
            draw_mesh(&Mesh { vertices, indices: vec![0,1,2,0,2,3], texture: Some(effect_textures[quad.texture as usize].clone()) });
        }
        gl_use_default_material();
        set_default_camera();
        draw_text("Pikmin pluck | NUKARERU 6 / NUKARE fast 83 | AnimInfo 29 / 93", 18., 27., 24., WHITE);
        draw_text("Dirt: particle emitters 50 sd_rakk1 + 51 sd_rakk2", 18., 54., 23., WHITE);
        draw_text("pikiState.cpp PikiNukareState::procAnimMsg | interactPullout.cpp", 18., 81., 21., WHITE);
        draw_text(&format!("Tick {tick}: {status}"), 18., 108., 22., WHITE);
        if tick == last {
            if let Some(path) = shot { get_screen_data().export_png(path); }
            break;
        }
        next_frame().await;
    }
}
