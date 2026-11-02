//! `max_age` is a host extension on the landing cloud: it must dim the puffs without
//! resizing them. Shortening `life` re-scores `age / life`, and `rkt_land.pcr`'s authored
//! scale curve (`min_scales[1] = 2.4`) then inflates every puff for the fade.

use std::sync::Arc;

use glam::Vec3;
use libpikmin_assets::Assets;
use libpikmin_assets::particle::{Generator, Pcr};

#[test]
fn the_max_age_fade_does_not_inflate_the_authored_scale() {
    let Some(assets) = Assets::discover(None) else {
        eprintln!("SKIP: no extracted assets");
        return;
    };
    let data = assets.read("effects/pcr/rkt_land.pcr").expect("rkt_land.pcr");
    let pcr = Arc::new(Pcr::parse(&data).expect("parse rkt_land.pcr"));
    let mut g = Generator::new(pcr, Vec3::ZERO, 1);
    g.max_age = Some(10.0);
    for _ in 0..10 {
        assert!(g.update(1.0, &mut |_| {}));
    }
    let scale_before = g.particles.iter().map(|p| p.scale).fold(0.0, f32::max);
    let alpha_before = g.particles.iter().map(|p| p.alpha).fold(0.0, f32::max);
    assert!(scale_before > 1.0);
    g.update(1.0, &mut |_| {});
    let scale_after = g.particles.iter().map(|p| p.scale).fold(0.0, f32::max);
    let alpha_after = g.particles.iter().map(|p| p.alpha).fold(0.0, f32::max);
    assert!((scale_after - scale_before).abs() < 0.02,
        "the fade must not inflate: {scale_before} -> {scale_after}");
    assert!(alpha_after < alpha_before, "the fade must dim: {alpha_before} -> {alpha_after}");
    let mut steps = 0;
    while g.update(1.0, &mut |_| {}) {
        steps += 1;
        assert!(steps < 10, "the fade must finish");
    }
}
