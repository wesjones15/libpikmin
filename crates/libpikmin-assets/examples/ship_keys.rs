//! Prints the ship's and ship parts' motions with their keys, for checking the upgrade
//! and part animations. `cargo run -p libpikmin-assets --example ship_keys`
use libpikmin_assets::{AnimMgr, AnimSet, Assets, ship};

fn dump(a: &Assets, mgr: &str, bundle: &str) {
    let m = AnimMgr::parse(&a.read(mgr).unwrap()).unwrap();
    let set = AnimSet::load(a, mgr, bundle).ok();
    for (i, info) in m.infos.iter().enumerate() {
        let frames = set.as_ref().and_then(|s| s.get(i)).map(|(_, an)| (an.frames, an.joints.len()));
        println!(
            "  {i} {} keys {:?} info {:?} events {:?} -> {:?}",
            info.name, info.keys, info.info_keys, info.event_keys, frames
        );
    }
}

fn main() {
    let a = Assets::discover(None).unwrap();
    println!("ship");
    dump(&a, ship::SHIP_ANIM_MGR, ship::SHIP_BUNDLE);
    let m = a.model(ship::SHIP_MODEL).unwrap();
    for j in [0usize, 38, 36, 19, 32, 37, 29, 53, 15, 52, 51, 40] {
        let mats: Vec<_> = m.joints[j].matpolys.iter().map(|p| p.0).collect();
        println!("joint {j} parent {} mats {:?}", m.joints[j].parent, mats);
    }
    for (i, mat) in m.materials.iter().enumerate() {
        let regs: Vec<_> = mat
            .tev_info
            .and_then(|t| m.tev_infos.get(t))
            .map(|t| t.regs.iter().map(|r| r.anim.frames).collect())
            .unwrap_or_default();
        if mat.color_anim.frames > 0 || regs.iter().any(|&f| f > 0) {
            println!("mat {i} colour frames {} reg frames {:?}", mat.color_anim.frames, regs);
        }
    }
    let infos = ship::parse_pel_anim(&a.read(ship::PEL_ANIM).unwrap()).unwrap();
    for (i, id) in ship::PART_IDS.iter().enumerate().take(16) {
        let Some(info) = infos.iter().find(|x| x.id == *id) else { continue };
        println!("part {i} {id} {} override {}", info.file, info.override_joint);
        dump(&a, &info.anim_mgr(), &info.bundle());
    }
}
