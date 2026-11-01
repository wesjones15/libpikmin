//! The Onion's leg ends while it flies: the same measurement as `onion_legs.rs` (each leg mesh's
//! lowest vertex, the foot, and the vertex nearest the pod), but in the take-off clip's airborne
//! loop (motion 7, frames 200-260) and rebased onto the host's trajectory the way the view does.
//! These are the ends a Pikmin climbing a *flying* Onion should hold.
use glam::Mat4;
use libpikmin_assets::{AnimSet, Assets, Skeleton, Skin, paths};

fn main() {
    let a = Assets::discover(None).unwrap();
    let m = a.model(paths::ONION).unwrap();
    let skel = Skeleton::new(&m);
    let set = AnimSet::load(&a, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE).unwrap();
    let (_, takeoff) = set.get(7).unwrap(); // c_red_t, red take-off
    let (_, idle) = set.get(1).unwrap(); // flowwait, the landed Rest the view rebases against
    let rest = skel.pose(Mat4::IDENTITY, Some((idle, 0.0)));
    // The view's hover loop is frames 200-260 (`animator::onion_motion`).
    let mut sums = [(glam::Vec3::ZERO, glam::Vec3::ZERO); 3];
    let mut n = 0.0;
    for step in 0..=6 {
        let frame = 200.0 + step as f32 * 10.0;
        let pose = skel.pose(Mat4::IDENTITY, Some((takeoff, frame)));
        let offset = rest.joints[1].w_axis.truncate() - pose.joints[1].w_axis.truncate();
        for (l, joints) in [[3u16, 4, 5], [6, 7, 8], [9, 10, 11]].into_iter().enumerate() {
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            let (mut foot, mut top) = (glam::Vec3::ZERO, glam::Vec3::ZERO);
            for b in m.batches().unwrap() {
                for v in &b.vertices {
                    let Skin::Joint(j) = v.skin else { continue };
                    if !joints.contains(&j) {
                        continue;
                    }
                    let p = pose.skin(v).0 + offset;
                    if p.y < lo {
                        lo = p.y;
                        foot = p;
                    }
                    if p.y > hi {
                        hi = p.y;
                        top = p;
                    }
                }
            }
            sums[l].0 += foot;
            sums[l].1 += top;
        }
        n += 1.0;
    }
    for (l, (foot, top)) in sums.into_iter().enumerate() {
        let (foot, top) = (foot / n, top / n);
        println!("leg {l}: foot ({:.1}, {:.1}, {:.1}), top ({:.1}, {:.1}, {:.1})",
            foot.x, foot.y, foot.z, top.x, top.y, top.z);
    }
}
