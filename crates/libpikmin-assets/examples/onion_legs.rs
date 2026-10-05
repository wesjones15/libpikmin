//! Onion leg geometry from the model: for each leg, the mesh's lowest vertex (the foot) and the
//! vertex nearest the pod.
use glam::Mat4;
use libpikmin_assets::{Assets, Skeleton, Skin, paths};
fn main() {
    let a = Assets::discover(None).unwrap();
    let m = a.model(paths::ONION).unwrap();
    let pose = Skeleton::new(&m).pose(Mat4::IDENTITY, None);
    for (leg, joints) in [(0, [3u16, 4, 5]), (1, [6, 7, 8]), (2, [9, 10, 11])] {
        let mut lo: Option<glam::Vec3> = None;
        let mut hi: Option<glam::Vec3> = None;
        for b in m.batches().unwrap() {
            for v in &b.vertices {
                let on_leg = match v.skin { Skin::Joint(j) => joints.contains(&j), _ => false };
                if !on_leg { continue; }
                let p = pose.skin(v).0;
                if lo.is_none_or(|l| p.y < l.y) { lo = Some(p); }
                if hi.is_none_or(|h| p.y > h.y) { hi = Some(p); }
            }
        }
        println!("leg {leg}: foot {:?} top {:?}", lo, hi);
    }
}
