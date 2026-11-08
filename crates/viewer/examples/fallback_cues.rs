//! Authored circles/pellet shapes, no extracted assets or original cue/model IDs.
//! Cursor transforms are the real View::captains_only / lock_view draw contract.
//! cargo run --release -p viewer --example fallback_cues -- <output-dir>
use libpikmin::{Sim, Params, PikiColor, PelletDesc, Vec3 as GVec3};
use libpikmin_present::{View, view::kinds};
use macroquad::prelude::*;
fn config()->Conf{Conf{window_title:"Asset-independent gameplay cues".into(),window_width:1100,window_height:760,..Default::default()}}
fn point(v:GVec3)->Vec3{vec3(v.x,v.y,v.z)}
fn ring(at:Vec3,rot: ::glam::Quat,r:f32,color:Color){for i in 0..64{let a=i as f32*std::f32::consts::TAU/64.;let b=(i+1)as f32*std::f32::consts::TAU/64.;
    draw_line_3d(at+point(rot*GVec3::new(a.cos(),0.,a.sin()))*r,at+point(rot*GVec3::new(b.cos(),0.,b.sin()))*r,color);}}
#[macroquad::main(config)]
async fn main(){
    let out=std::env::args().nth(1).expect("output directory");std::fs::create_dir_all(&out).unwrap();
    let params=Params::default();let mut sim=Sim::new(params.clone(),4);let c=sim.add_captain(GVec3::ZERO);
    let mut target=None;
    for (i,(n,col)) in [(1,PikiColor::Blue),(5,PikiColor::Red),(10,PikiColor::Yellow),(20,PikiColor::Blue)].into_iter().enumerate(){
        let p=sim.spawn_pellet(PelletDesc::numbered(&params,n,col,GVec3::new((i as f32-1.5)*100.,0.,170.),0.));if i==1{target=Some(p);}}
    let mut view=View::captains_only();
    for frame in 0..=122{
        if frame==60{sim.set_lock_pellet(c,target);}
        sim.advance(&mut libpikmin::host::FlatHost,1./60.);view.update(&sim,sim.alpha(),1./60.);
        clear_background(Color::from_rgba(23,29,40,255));
        set_camera(&Camera3D{position:vec3(9.,9.,13.),target:vec3(0.,1.,3.),up:Vec3::Y,..Default::default()});draw_grid(20,1.,GRAY,DARKGRAY);
        draw_cylinder(vec3(0.,0.9,0.),0.35,0.35,1.8,None,GOLD);
        for pel in sim.pellets(){let rgb=libpikmin_present::animator::piki_rgb(pel.desc.color.unwrap());let r=pel.desc.radius*pel.scale*0.04;
            draw_cylinder(point(pel.pos)*0.04+Vec3::Y*r*0.35,r,r,r*0.7,None,Color::from_rgba(rgb[0],rgb[1],rgb[2],255));}
        let draw=view.draws.iter().find(|d|d.kind==kinds::CURSOR).unwrap();
        ring(point(draw.pos)*0.04,draw.rot,0.35,PINK);
        // Mario's authored circle uses the same world-facing plane as MarkerQuad.
        let at=vec3(4.,2.,0.);let eye=GVec3::new(0.,1.,0.);let normal=(eye-GVec3::new(at.x,at.y,at.z)).normalize();
        let x=GVec3::Y.cross(normal).normalize();let z=x.cross(normal);let rot=::glam::Quat::from_mat3(&::glam::Mat3::from_cols(x,normal,z));ring(at,rot,0.3,YELLOW);
        set_default_camera();draw_text("PROCEDURAL CUES | no original asset / effect / model IDs",24.,36.,24.,WHITE);
        draw_text("Actual native Olimar cursor pose | same simulated pellet colors, radii and scale",24.,66.,22.,LIGHTGRAY);
        draw_text(if frame<60{"FREE: pink circle on ground"}else{"LOCKED: pink circle faces Olimar"},24.,710.,24.,PINK);
        draw_text("Mario: yellow facing circle",700.,710.,21.,YELLOW);
        if [30,90,120].contains(&frame){get_screen_data().export_png(&format!("{out}/cues-{frame}.png"));}next_frame().await;
    }
}
