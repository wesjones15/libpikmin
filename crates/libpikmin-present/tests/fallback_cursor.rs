use libpikmin::{Params,Sim,Vec3,PelletDesc,PikiColor,TICK_DT};
use libpikmin_present::{View,view::kinds};
fn cursor(view:&View)->&libpikmin_present::view::Draw{view.draws.iter().find(|d|d.kind==kinds::CURSOR).unwrap()}
#[test]
fn empty_view_retains_free_locked_spinning_and_released_cursor_poses(){
    let params=Params::default();let mut sim=Sim::new(params.clone(),5);
    let captain=sim.add_captain(Vec3::ZERO);
    let pellet=sim.spawn_pellet(PelletDesc::numbered(&params,5,PikiColor::Red,Vec3::Z*100.,0.));
    let mut view=View::captains_only();
    view.update(&sim,1.,TICK_DT);
    assert!((cursor(&view).rot*Vec3::Y).dot(Vec3::Y)>0.999);
    sim.set_lock_pellet(captain,Some(pellet));sim.step(&mut libpikmin::host::FlatHost);
    view.update(&sim,1.,TICK_DT);
    let lock=sim.captain(captain).lock_view.expect("pellet did not lock");
    let facing=(lock.eye-cursor(&view).pos).normalize();
    assert!((cursor(&view).rot*Vec3::Y).dot(facing)>0.999,"stand-in cursor did not face captain");
    assert!(cursor(&view).pos.distance(lock.pos)<0.001,"stand-in cursor lost native surface placement");
    let first=cursor(&view).rot;
    for _ in 0..10{view.update(&sim,1.,TICK_DT);}
    assert!(first.angle_between(cursor(&view).rot)>0.2,"lock spin was gated on textures");
    sim.set_lock_pellet(captain,None);sim.step(&mut libpikmin::host::FlatHost);view.update(&sim,1.,TICK_DT);
    assert!((cursor(&view).rot*Vec3::Y).dot(Vec3::Y)>0.999,"release did not restore ground plane");
}

#[test]
fn cursor_transform_matches_original_view_when_cache_is_available(){
    let Some(assets)=libpikmin_assets::Assets::discover(None) else{eprintln!("SKIP: no optional Pikmin cache for original/stand-in cursor comparison");return};
    let mut original=View::load(&assets).unwrap();let mut fallback=View::captains_only();
    let params=Params::default();let mut sim=Sim::new(params.clone(),8);let c=sim.add_captain(Vec3::ZERO);
    let pellet=sim.spawn_pellet(PelletDesc::numbered(&params,10,PikiColor::Yellow,Vec3::new(40.,0.,80.),0.));
    for frame in 0..30{
        if frame==5{sim.set_lock_pellet(c,Some(pellet));}if frame==20{sim.set_lock_pellet(c,None);}
        sim.step(&mut libpikmin::host::FlatHost);
        for view in [&mut original,&mut fallback]{view.update(&sim,0.5,TICK_DT);}
        assert!(cursor(&original).pos.distance(cursor(&fallback).pos)<0.0001,"asset presence changed cursor placement");
        assert!(cursor(&original).rot.dot(cursor(&fallback).rot).abs()>0.99999,"asset presence changed cursor rotation");
    }
}
