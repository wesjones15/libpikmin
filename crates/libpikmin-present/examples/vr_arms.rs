//! VR arms proof for every captain body (`vr_arms::bodies`): Olimar, Louie, the two Piantas and Captain Pikachu.
//! One picture per body (libcommon's `vr_arms::capture`): front and side in the bind pose, the right arm red and the
//! left blue, then each arm alone in its arm space. A body whose cache is missing prints SKIP.
//!   cargo run --release -p libpikmin-present --example vr_arms -- <assets> <sms assets> <p2 cache> <melee cache> <out dir>

use std::path::PathBuf;

use libcommon::vr_arms;
use libpikmin_present::View;
use libpikmin_present::view::models;
use libpikmin_present::vr_arms::{Sources, bodies};

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let dir = args.next().unwrap_or_else(|| "assets".into());
    let sms = args.next().unwrap_or_else(|| "../libsms/assets".into());
    let p2 = args.next().unwrap_or_else(|| dir.join("p2"));
    let melee = args.next().unwrap_or_else(|| "../libssbm/assets".into());
    let out = args.next().unwrap_or_else(|| "vr_arms".into());
    if !dir.join("pikis").is_dir() {
        println!("SKIP: no Pikmin assets at {}", dir.display());
        return;
    }
    std::fs::create_dir_all(&out).unwrap();
    let view = View::load(&libpikmin_assets::Assets { root: dir.clone() }).unwrap();
    let src = Sources { sms: Some(&sms), p2: Some(&p2), melee: Some(&melee) };
    for body in bodies(&view.models[models::OLIMAR as usize], &dir, src) {
        let (mesh, arms, made) = match body {
            Ok(b) => b,
            Err(e) => { println!("SKIP {e}"); continue; }
        };
        let (w, h, px) = vr_arms::capture(&mesh, &arms, &format!("PIKMIN {}", arms.name));
        let path = out.join(format!("{}.png", arms.name));
        std::fs::write(&path, vr_arms::png(w, h, &px)).unwrap();
        let tris: Vec<usize> = arms.arms.iter().map(|a| a.triangles.len()).collect();
        println!("{}: {} arms {tris:?} of {}{} -> {}", arms.name, arms.arms.len(), mesh.triangles.len(),
            if made { " (cut now)" } else { "" }, path.display());
    }
}
