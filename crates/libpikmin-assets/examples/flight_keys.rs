//! Inspect extracted Onion flight poses and their model-space displacement.
use glam::Mat4;
use libpikmin_assets::{AnimMgr, AnimSet, Assets, Skeleton, paths};
fn main() {
    let assets = Assets::discover(None).unwrap();
    let mgr = AnimMgr::parse(&assets.read(paths::ONION_ANIM_MGR).unwrap()).unwrap();
    let set = AnimSet::load(&assets, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE).unwrap();
    let model = assets.model(paths::ONION).unwrap();
    let skel = Skeleton::new(&model);
    for (index, info) in mgr.infos.iter().enumerate() {
        println!("{index}: {} {:?}", info.name, info.event_keys);
        let Some((_, anim)) = set.get(index) else { continue };
        for frame in [0.0, 20.0, 40.0, 80.0, 120.0, 160.0, 200.0, 240.0, 278.0, 320.0, 397.0] {
            if frame >= anim.frames as f32 { continue; }
            let pose = skel.pose(Mat4::IDENTITY, Some((anim, frame)));
            println!("  {frame}: root {:?}, body {:?}, flower {:?}",
                pose.joints[0].w_axis.truncate(), pose.joints[1].w_axis.truncate(), pose.joints[3].to_scale_rotation_translation().1);
        }
    }
}
