use libpikmin::{Params, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::kinds;

#[test]
fn healing_adds_one_soul_per_sacrifice_without_relocating_olimar_or_repeating() {
    let Some(assets) = Assets::discover(None) else { return };
    let mut sim = Sim::new(Params::default(), 1);
    let captain = sim.add_captain(Vec3::new(90.0, 0.0, 50.0));
    let mut bare = View::load(&assets).unwrap();
    let mut souls = View::load(&assets).unwrap();
    bare.set_healing(captain.0, 0.0, 0);
    souls.set_healing(captain.0, 0.0, 4);
    bare.update(&sim, 1.0, TICK_DT);
    souls.update(&sim, 1.0, TICK_DT);
    let base = bare.effects.as_ref().unwrap().running();
    let count = souls.effects.as_ref().unwrap().running();
    assert!(base >= 3, "Dolphin recovery burst, sparkles and rings are live");
    assert_eq!(count, base + 4, "one original Pikmin soul generator per sacrifice");
    let olimar = souls.draws.iter().find(|d| d.kind == kinds::CAPTAIN).unwrap();
    assert!(olimar.pos.distance(sim.captain(captain).pos) < 0.001, "healing stays at Olimar, not the Dolphin hatch");
    souls.set_healing(captain.0, 0.1, 4);
    souls.update(&sim, 1.0, 0.0);
    assert_eq!(souls.effects.as_ref().unwrap().running(), count, "rendering another frame does not create more ghosts");
    souls.set_healing(captain.0, -1.0, 0);
    for _ in 0..450 { souls.update(&sim, 1.0, TICK_DT); }
    assert_eq!(souls.effects.as_ref().unwrap().running(), 0, "healing effects finish after the animation ends");
}
