use libpikmin::{Params, Sim, Vec3, PikiColor, TICK_DT};
use libpikmin::host::FlatHost;
use libpikmin_assets::Assets;
use libpikmin_present::{View, view::kinds};

fn bounds(v: &View, kind: u8) -> (Vec3, Vec3) {
    let draw = v.draws.iter().find(|d| d.kind == kind).unwrap();
    let vertices = &v.positions[draw.first_vertex as usize..][..v.models[draw.model as usize].vertex_count];
    vertices.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), p| { assert!(p.is_finite()); (lo.min(*p), hi.max(*p)) })
}

#[test]
fn dolphin_beam_ends_with_a_visible_full_size_captain_at_his_feet() {
    let Some(assets) = Assets::discover(None) else { return };
    let mut v = View::load(&assets).unwrap();
    let mut s = Sim::new(Params::default(), 1);
    let c = s.add_captain(Vec3::ZERO);
    s.add_ship(Vec3::Z * -75.0, Vec3::Z * 25.0, Vec3::new(0.0, 74.0, -75.0));
    s.step(&mut FlatHost);
    v.update(&s, 1.0, TICK_DT);
    let (lo, hi) = bounds(&v, kinds::CAPTAIN);
    let height = hi.y - lo.y;
    for progress in [0.0, 0.1, 0.5, 0.9, 1.0, -1.0] {
        v.set_materialize(c.0, progress);
        v.update(&s, 1.0, TICK_DT);
        let draw = v.draws.iter().find(|d| d.kind == kinds::CAPTAIN).unwrap();
        assert_eq!(draw.alpha, 1.0);
        let (lo, hi) = bounds(&v, kinds::CAPTAIN);
        assert!(hi.y > lo.y, "never collapses to zero vertices");
        if progress == 0.0 { assert!(draw.pos.y > 70.0 && draw.pos.z < -70.0, "starts at the Dolphin hatch"); }
        if progress >= 1.0 || progress < 0.0 {
            assert!(draw.pos.distance(s.captain(c).pos) < 0.001);
            assert!(hi.y - lo.y > height * 0.95, "full size restored");
        }
    }
}

#[test]
fn every_dormant_colour_folds_and_replays_its_disc_effects_and_sound_track() {
    let Some(assets) = Assets::discover(None) else { return };
    for (colour, demo) in [(PikiColor::Blue, 6), (PikiColor::Red, 4), (PikiColor::Yellow, 5)] {
        let mut s = Sim::new(Params::default(), 1);
        let o = s.add_onion(Vec3::ZERO, &[colour]);
        let mut v = View::load(&assets).unwrap();
        v.update(&s, 1.0, TICK_DT);
        let (lo, hi) = bounds(&v, kinds::ONION);
        let live_height = hi.y - lo.y;
        s.make_onion_dormant(o, 2.0, Vec3::ZERO);
        for _ in 0..46 { s.step(&mut FlatHost); v.update(&s, 1.0, TICK_DT); }
        let (lo, hi) = bounds(&v, kinds::ONION);
        // The original startup buries its long legs under the ground plane; its pod
        // rests on the surface. Do not raise the buried leg vertices above ground.
        assert!(hi.y < live_height * 0.8, "{colour:?}: dormant pod near the ground, {lo:?}..{hi:?}");
        assert!(!v.draws.iter().any(|d| d.kind == kinds::BEAM), "dormant light is off");
        for _ in 0..120 { s.step(&mut FlatHost); v.update(&s, 1.0, TICK_DT); }
        assert!(s.onions()[0].dormancy.is_none());
        assert!(v.demo_sounds.iter().any(|s| *s == Some((demo, 0))), "starts the correct discovery sound");
        assert!(v.demo_sounds.iter().any(|s| *s == Some((demo, 7))), "completes its sound commands");
        assert!(v.demo_sounds.contains(&None), "stops the cutscene track");
        v.particles(Vec3::X, Vec3::Y);
        let (lo, hi) = bounds(&v, kinds::ONION);
        assert!(hi.y - lo.y > live_height * 0.9, "{colour:?}: live shape restored");
    }
}
