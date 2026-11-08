//! Exact optional bodies without Pikmin 1 assets. No effect cue is involved.
//! Pianta: npc/montem/mom_model.bmd, libsms-present/body.rs::pianta_files.
//! Pikachu: PlPkNr.dat, libpikmin-present/pikachu.rs::PikachuSkin::load.
use libpikmin::{Sim, Params, CaptainInput, Vec2 as GVec2, Vec3 as GVec3};
use libpikmin_present::View;
use macroquad::prelude::*;
fn config() -> Conf { Conf { window_title: "Independent captain assets".into(), window_width:1000, window_height:760, ..Default::default() } }
#[macroquad::main(config)]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mut view = View::captains_only();
    let (skin, model, label) = if args[1] == "pianta" { (2, view.load_pianta(args[2].as_ref(), false, 0).unwrap(), "Pianta mom_model.bmd | body.rs::pianta_files") }
        else { (3, view.load_pikachu(args[2].as_ref()).unwrap(), "Pikachu PlPkNr.dat | pikachu.rs::PikachuSkin::load") };
    let mut sim = Sim::new(Params::default(),9); let captain = sim.add_captain(GVec3::ZERO);
    sim.set_input(captain, CaptainInput { stick:GVec2::X, ..Default::default() });
    view.set_captain_skin(captain.0, skin);
    std::fs::create_dir_all(&args[3]).unwrap();
    let textures:Vec<_> = view.textures.list.iter().map(|t| Texture2D::from_rgba8(t.width as u16,t.height as u16,&t.rgba)).collect();
    for frame in 0..=62 {
        sim.advance(&mut libpikmin::host::FlatHost,1./60.); view.update(&sim,sim.alpha(),1./60.);
        clear_background(Color::from_rgba(25,30,40,255));
        let feet = sim.captain(captain).pos;
        set_camera(&Camera3D { position:vec3(70.,50.,90.), target:vec3(0.,15.,0.), up:Vec3::Y, ..Default::default() });
        draw_grid(20,5.,GRAY,DARKGRAY);
        for draw in view.draws.iter().filter(|d| d.model == model) {
            let mut v=draw.first_vertex as usize;
            for (bi,batch) in view.models[draw.model as usize].batches.iter().enumerate() {
                let c=view.colors[draw.first_color as usize+bi];
                let vertices:Vec<_>=batch.vertices.iter().enumerate().map(|(i,_)| {
                    let p=draw.pos-feet+draw.rot*view.positions[v+i];let uv=view.uvs[v+i];
                    Vertex::new(p.x,p.y,p.z,uv[0],uv[1],Color::new(c[0],c[1],c[2],c[3])) }).collect();
                let t=view.texture_ids[draw.first_color as usize+bi];
                for chunk in batch.indices.chunks(1500) { draw_mesh(&Mesh { vertices:chunk.iter().map(|i|vertices[*i as usize]).collect(), indices:(0..chunk.len() as u16).collect(),texture:(t>=0).then(||textures[t as usize].clone()) }); } v+=batch.vertices.len();
            }
        }
        set_default_camera();draw_text(&format!("{label} | model {model} | frame {frame}"),20.,32.,20.,WHITE);
        draw_text("No Pikmin 1 assets. Generated anchor rig; alternate body's own animation.",20.,60.,20.,WHITE);
        if [30,45,60].contains(&frame) {get_screen_data().export_png(&format!("{}/{}-{frame}.png",args[3],args[1]));}
        next_frame().await;
    }
}
