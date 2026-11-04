//! The view's VR arms (`View::set_vr_arms`): one draw a side for the chosen captain only, in arm space (the hand's
//! middle at the origin), Olimar-sized, and none once switched off.

use libpikmin::host::FlatHost;
use libpikmin::{Params, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::kinds;

#[test]
fn the_vr_captain_gets_an_arm_draw_a_side_in_arm_space() {
    let Some(assets) = Assets::discover(None) else {
        eprintln!("SKIP: no Pikmin 1 assets");
        return;
    };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 35);
    let c = sim.add_captain(Vec3::ZERO);
    let other = sim.add_captain(Vec3::new(200.0, 0.0, 0.0));
    view.set_vr_arms(Some(c.0), &assets.root);
    for _ in 0..3 {
        sim.step(&mut FlatHost);
        view.update(&sim, 1.0, TICK_DT);
    }
    let arms: Vec<_> = view.draws.iter().filter(|d| d.kind == kinds::VR_ARM).copied().collect();
    assert_eq!(arms.len(), 2, "one a side for the VR captain only (not captain {})", other.0);
    let body = view.draws.iter().find(|d| d.kind == kinds::CAPTAIN).unwrap();
    let next = view.draws.iter().map(|d| d.first_vertex).filter(|&v| v > body.first_vertex).min().unwrap();
    let count = (next - body.first_vertex) as usize;
    let height = view.positions[body.first_vertex as usize..][..count].iter().map(|p| p.y).fold(0.0, f32::max);
    for arm in &arms {
        let kept: Vec<Vec3> = view.positions[arm.first_vertex as usize..][..count].iter().copied().filter(|p| *p != Vec3::ZERO).collect();
        assert!(!kept.is_empty(), "the cut keeps the arm's vertices");
        let reach = kept.iter().map(|p| p.length()).fold(0.0, f32::max);
        assert!(reach.is_finite() && reach > 0.5 && reach < height, "an arm {reach} long, him {height} tall");
    }

    view.set_vr_arms(None, &assets.root);
    sim.step(&mut FlatHost);
    view.update(&sim, 1.0, TICK_DT);
    assert!(view.draws.iter().all(|d| d.kind != kinds::VR_ARM));
}
