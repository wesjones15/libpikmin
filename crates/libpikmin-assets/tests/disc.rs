//! Checks against the user's own disc. Skipped when no disc is found (set `PIKMIN_DISC`).

use glam::{Mat4, Vec3};
use libpikmin_assets::motion::Motion;
use libpikmin_assets::{AnimSet, Disc, Model, Skeleton, paths};

fn disc() -> Option<Disc> {
    let path = libpikmin_assets::find_disc()?;
    Some(Disc::open(&path).expect("disc opens"))
}

fn bounds(model: &Model, skel: &Skeleton, anim: Option<(&libpikmin_assets::Anim, f32)>) -> (Vec3, Vec3) {
    let pose = skel.pose(Mat4::IDENTITY, anim);
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for b in model.batches().unwrap() {
        for v in &b.vertices {
            let (p, n) = pose.skin(v);
            assert!(p.is_finite() && n.is_finite());
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo, hi)
}

#[test]
fn every_model_and_animation_loads() {
    let Some(disc) = disc() else { return };
    assert_eq!(disc.game_id, "GPIE01");
    let mut all = vec![paths::OLIMAR, paths::ONION];
    all.extend(paths::PIKMIN);
    all.extend(paths::HAPPA);
    all.extend(paths::PELLETS);
    for p in all {
        let m = Model::parse(&disc.read(p).unwrap()).unwrap();
        assert!(!m.batches().unwrap().is_empty(), "{p}");
        for t in &m.textures {
            t.decode().unwrap();
        }
    }
    let set = AnimSet::load(&disc, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE).unwrap();
    // Every motion the table names resolves to animation data.
    for i in 0..90u8 {
        if let Some(idx) = libpikmin_assets::motion::MOTION_TABLE[i as usize] {
            assert!(set.get(idx as usize).is_some(), "motion {i} -> info {idx}");
        }
    }
    let goal = AnimSet::load(&disc, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE).unwrap();
    assert!(goal.anims.iter().all(Option::is_some));
}

#[test]
fn pikmin_stands_upright_at_sim_scale() {
    let Some(disc) = disc() else { return };
    let set = AnimSet::load(&disc, paths::PIKI_ANIM_MGR, paths::PIKI_BUNDLE).unwrap();
    let wait = set.get(Motion::Wait.anim_index().unwrap()).unwrap().1;
    for p in [paths::PIKMIN[0], paths::OLIMAR] {
        let m = Model::parse(&disc.read(p).unwrap()).unwrap();
        let skel = Skeleton::new(&m);
        let (lo, hi) = bounds(&m, &skel, Some((wait, 0.0)));
        let height = hi.y - lo.y;
        println!("{p}: {lo} .. {hi}");
        assert!(lo.y > -3.0 && (10.0..40.0).contains(&height), "{p}: {lo} .. {hi}");
    }
}
