//! Prints teki models, animations and parameters. `cargo run -p libpikmin-assets --example teki`
use libpikmin_assets::teki::{TekiParams, teki_paths, tpf};
use libpikmin_assets::{AnimSet, Assets, paths};

fn main() {
    let a = Assets::discover(None).unwrap();
    for (name, ints) in [("chappy", 21), ("swallow", 20), ("qurione", 20)] {
        let [m, anm, key, para] = teki_paths(name);
        let model = a.model(&m).unwrap();
        let set = AnimSet::load(&a, &key, &anm).unwrap();
        let p = TekiParams::parse_any(&a.read(&para).unwrap(), ints).unwrap();
        println!("{name}: {} joints {:?} tris {}", model.joints.len(), model.joint_names, model.batches().unwrap().iter().map(|b| b.indices.len() / 3).sum::<usize>());
        println!("  params v{} ints {:?}", p.version, p.ints);
        println!("  life {} scale {} walk {} run {} turn {} vis {} angle {} atkable {} atkangle {} atkrange {} danger {} safety {} coll {} lower {}", p.f(tpf::LIFE), p.f(tpf::SCALE), p.f(tpf::WALK_VELOCITY), p.f(tpf::RUN_VELOCITY), p.f(tpf::TURN_VELOCITY), p.f(tpf::VISIBLE_RANGE), p.f(tpf::VISIBLE_ANGLE), p.f(tpf::ATTACKABLE_RANGE), p.f(tpf::ATTACKABLE_ANGLE), p.f(tpf::ATTACK_RANGE), p.f(tpf::DANGER_TERRITORY_RANGE), p.f(tpf::SAFETY_TERRITORY_RANGE), p.f(tpf::COLLISION_RADIUS), p.f(tpf::LOWER_RANGE));
        println!("  specific floats {:?}", &p.floats[50..]);
        for (i, info) in set.mgr.infos.iter().enumerate() {
            println!("  motion {i} {} keys {:?} info {:?} -> {:?}", info.name, info.keys, info.info_keys, set.anims[i].as_ref().map(|x| (x.frames, x.joints.len())));
        }
    }
    for (m, b, k) in [(paths::PIKIHEAD, paths::PIKIHEAD_BUNDLE, paths::PIKIHEAD_ANIM_MGR), (paths::WATER, paths::WATER_BUNDLE, paths::WATER_ANIM_MGR), (paths::FALL_WATER, paths::FALL_WATER_BUNDLE, paths::FALL_WATER_ANIM_MGR)] {
        let model = a.model(m).unwrap();
        let set = AnimSet::load(&a, k, b).unwrap();
        println!("{m}: {} joints", model.joints.len());
        for (i, info) in set.mgr.infos.iter().enumerate() {
            println!("  motion {i} {} keys {:?} info {:?} -> {:?}", info.name, info.keys, info.info_keys, set.anims[i].as_ref().map(|x| (x.frames, x.joints.len())));
        }
    }
}
