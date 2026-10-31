//! Viewer-first proof of kind 23: palm.mod / palm.key, TaiPalmStrategy.
//! cargo run --release -p viewer --example pellet_posy -- <capture.png> <tick>
use ::glam::Vec3 as GVec3;
use libpikmin::{Params, Sim};
use libpikmin::host::FlatHost;
use libpikmin_assets::Assets;
use libpikmin_present::{View, teki::TekiInstance};
use libpikmin_teki::{Tekis, Out, info, opt};
use macroquad::prelude::*;

fn config() -> Conf { Conf { window_title: "Pellet Posy: source audit".into(), window_width: 1100, window_height: 760, ..Default::default() } }
fn texture(width: u32, height: u32, rgba: &[u8]) -> Texture2D {
    let t = Texture2D::from_rgba8(width as u16, height as u16, rgba);
    t.set_filter(FilterMode::Linear);
    t
}

#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let shot = args.get(1);
    let flint = args.iter().any(|a| a == "--flint");
    let last: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(90);
    let assets = Assets::discover(None).expect("extract your Pikmin disc first");
    libpikmin_present::view::set_teki_models(true);
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 9);
    let mut tekis = Tekis::load(&assets);
    let mut ids = Vec::new();
    for size in 0..if flint {1} else {3} {
        let id = tekis.spawn(if flint {info::P1_FLINT}else{info::PELLET_POSY}, size + 1, GVec3::new((size as f32 - 1.0) * 110.0, 0.0, 0.0), 0.0).unwrap();
        let t = tekis.get_mut(id).unwrap();
        if !flint { t.count = [size as i32, size as i32, 1, 0]; }
        ids.push(id);
    }
    let facts = view.teki_info(if flint {info::P1_FLINT}else{info::PELLET_POSY}).unwrap();
    println!("asset facts: life {} bounds {:?}..{:?}", facts.life, facts.lo, facts.hi);
    for (i, m) in facts.motion_data.iter().enumerate() { println!("motion {i}: frames {} events {:?}", m.frames, m.events); }
    let eye = if flint { vec3(120.0, 140.0, 240.0) } else { vec3(210.0, 150.0, 390.0) };
    let target = if flint {vec3(0.0,35.0,0.0)} else {vec3(0.0, 40.0, 0.0)};
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward).normalize();
    let gv = |v: Vec3| GVec3::new(v.x, v.y, v.z);
    let digits:Vec<_>=(0..10).map(|d| {let i=libpikmin_assets::screen::number_texture(&assets,d,false).unwrap();texture(i.width,i.height,&i.rgba)}).collect();
    let mut textures = Vec::new();
    let mut effect_textures = Vec::new();
    let additive = load_material(ShaderSource::Glsl {
        vertex: "#version 100\nattribute vec3 position; attribute vec2 texcoord; attribute vec4 color0; uniform mat4 Model; uniform mat4 Projection; varying lowp vec4 c; varying mediump vec2 uv; void main(){gl_Position=Projection*Model*vec4(position,1.0); c=color0/255.0; uv=texcoord;}",
        fragment: "#version 100\nprecision lowp float; varying lowp vec4 c; varying mediump vec2 uv; uniform sampler2D Texture; void main(){gl_FragColor=texture2D(Texture,uv)*c;}",
    }, MaterialParams { pipeline_params: miniquad::PipelineParams {
        color_blend: Some(miniquad::BlendState::new(miniquad::Equation::Add, miniquad::BlendFactor::One, miniquad::BlendFactor::One)),
        depth_test: miniquad::Comparison::LessOrEqual, depth_write: false, ..Default::default()
    }, ..Default::default() }).unwrap();
    for tick in 0..=last {
        if flint && tick >= 30 && (tick-30)%60==0 { tekis.damage(ids[0], 1e6, None); tekis.press(ids[0]); }
        if !flint && tick == 30 { tekis.damage(ids[2], 0.2, None); }
        if !flint && tick == 70 { for &id in &ids[..2] { tekis.get_mut(id).unwrap().timers[0] = 0.0; } }
        if !flint && tick == 140 { tekis.get_mut(ids[0]).unwrap().timers[0] = 0.0; }
        if !flint && tick == 180 { tekis.damage(ids[2], 1e6, None); }
        sim.step(&mut FlatHost);
        for out in tekis.update(&mut sim, &FlatHost, &[libpikmin_teki::Navi{id:1,pos:GVec3::ZERO,size:8.0,alive:true}], 1.0 / 30.0) {
            match out {
                Out::Effect { id, pos } => { println!("tick {tick} effect {id}"); view.spawn_effect(id, pos); }
                Out::EffectDir { id, pos, dir } => { println!("tick {tick} oriented effect {id}"); view.spawn_effect_dir(id, pos, dir); }
                Out::TableSound { teki, index } => println!("tick {tick} sound teki {teki}, table {index} (asset sound base + index)"),
                Out::SpawnPellets { pos, number, color, count } => {
                    println!("tick {tick} pellet index {number} color {color}");
                    for _ in 0..count { sim.spawn_pellet(libpikmin::PelletDesc::numbered(sim.params(), [1,5,10,20][number as usize], libpikmin::PikiColor::ALL[color as usize], pos, 0.0)); }
                }
                _ => {}
            }
        }
        let instances: Vec<_> = tekis.list.iter().filter(|t| t.option(opt::VISIBLE)).map(|t| TekiInstance {
            id: t.id, kind: t.kind, pos: t.pos, yaw: t.yaw, motion: t.anim.motion as u32,
            frame: t.anim.counter, flash: t.flash, scale: if flint {t.fade} else {1.0}, alpha: 1.0, material_frame: if flint {None}else{Some(t.count[1] as f32)}, presentation:0,
        }).collect();
        if flint { for t in &mut tekis.list { t.pos=GVec3::ZERO; } }
        view.set_tekis(&instances);
        view.set_camera(gv(right), gv(up), gv(forward));
        view.update(&sim, 1.0, 1.0 / 30.0);
        view.particles(gv(right), gv(up));
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
        draw_text(if flint {"P1 Flint Beetle | kind 56 | bosses/kogane/kogane.mod + .anm"} else {"Pellet Posy | kind 23 | tekis/palm/palm.mod | palm.key motions 0-10"}, 18., 27., 24., WHITE);
        draw_text(if flint {"KoganeAi.cpp: initCreate / keyAction1 | particle emitters 19,87-89,120"} else {"taipalm.cpp: translateMotionIndex / createEffect | particle emitters 70-73"}, 18., 54., 23., WHITE);
        draw_text(&if flint {format!("Tick {tick}: walk / flip / burrow | Kogane sounds 0x76-0x77")} else {format!("Tick {tick}: growth / hit / drop | SE_FLOWER_DOWN..GROW2 (0x2E-0x32)")}, 18., 81., 22., WHITE);
        if flint {
            draw_text("HUD | zen/Number.h NumberPicCallBack | unchanged / +1 / -1",18.,610.,22.,WHITE);
            for (col,dir) in [0,1,-1].into_iter().enumerate() {
                let phase=(tick%30) as f32/30.0;
                let scale=libpikmin_present::hud_numbers::change_scale(dir,phase);
                for n in 0..3 {let s=if n==2 {scale}else{1.0};let x=180.0+col as f32*290.0+n as f32*44.0;draw_texture_ex(&digits[if n==2 {1}else{0}],x-22.0*s,670.0-22.0*s,WHITE,DrawTextureParams{dest_size:Some(vec2(44.0*s,44.0*s)),..Default::default()});}
            }
        }
        if tick == last {
            if let Some(path) = shot { get_screen_data().export_png(path); }
            break;
        }
        next_frame().await;
    }
}
