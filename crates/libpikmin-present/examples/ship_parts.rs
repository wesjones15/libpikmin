//! Which of the Dolphin's 30 parts have a model and a joint on the ship, and which the animator drives.
use libpikmin_assets::ship::{ship_joint, PART_IDS, parse_pel_anim, PEL_ANIM, SHIP_MODEL};
use libpikmin_assets::Assets;

fn main() {
    let assets = Assets::discover(None).expect("no assets");
    let infos = parse_pel_anim(&assets.read(PEL_ANIM).unwrap()).unwrap();
    let ship = assets.model(SHIP_MODEL).unwrap();
    for (i, id) in PART_IDS.iter().enumerate() {
        let info = infos.iter().find(|p| p.id == *id);
        let model = info.map(|p| assets.model(&p.model()).is_ok());
        let joint = info.and_then(|p| ship.joint_index(&ship_joint(&p.file)));
        println!("{i:2} {id} {} model {model:?} joint {joint:?}", info.map_or("?", |p| p.file.as_str()));
    }
}
