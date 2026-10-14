//! A captain beamed in is a small figure under a beam; with no beam (or a negative progress) he
//! is drawn at his normal size, never hidden.

use libpikmin::host::FlatHost;
use libpikmin::{Params, Sim, TICK_DT, Vec3};
use libpikmin_assets::Assets;
use libpikmin_present::View;
use libpikmin_present::view::kinds;

fn extent(view: &View) -> f32 {
    let d = view.draws.iter().find(|d| d.kind == kinds::CAPTAIN).unwrap();
    let n = view.models[d.model as usize].vertex_count;
    let pts = &view.positions[d.first_vertex as usize..][..n];
    let (lo, hi) = pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(l, h), p| (l.min(*p), h.max(*p)));
    (hi - lo).length()
}

#[test]
fn a_negative_progress_means_no_beam_and_forming_has_healing_effects() {
    let Some(assets) = Assets::discover(None) else { return };
    let mut view = View::load(&assets).unwrap();
    let mut sim = Sim::new(Params::default(), 3);
    let c = sim.add_captain(Vec3::ZERO);
    sim.step(&mut FlatHost);
    view.update(&sim, 1.0, TICK_DT);
    let normal = extent(&view);
    assert!(normal > 5.0);
    view.set_materialize(c.0, -1.0);
    view.update(&sim, 1.0, TICK_DT);
    assert!((extent(&view) - normal).abs() < normal * 0.03, "no beam: his full size");
    assert!(!view.draws.iter().any(|d| d.kind == kinds::BEAM));
    view.set_materialize(c.0, 0.3);
    view.update(&sim, 1.0, TICK_DT);
    assert!(extent(&view) < normal * 0.9, "forming");
    assert!(!view.draws.iter().any(|d| d.kind == kinds::BEAM), "no Onion beam over him");
    let fx = view.effects.as_ref().map(|e| e.running()).unwrap_or(0);
    assert!(fx > 0, "the healing effects run while he forms");
    view.set_materialize(c.0, 1.0);
    view.update(&sim, 1.0, TICK_DT);
    assert!((extent(&view) - normal).abs() < normal * 0.03, "done: his full size");
}
