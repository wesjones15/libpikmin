//! The Onion's drawn legs while it flies: each leg's hip, knee and foot joints in the take-off
//! clip's airborne loop (motion 7, frames 200-260), rebased onto the host's trajectory the way
//! the view does. The airborne loop bends at the knee, so a straight hip-to-foot rope leaves the
//! drawn leg; the host follows this polyline instead.
//!
//! These joints feed `libpikmin::onion_menu::HOVER_LEG_JOINTS`.
use glam::Mat4;
use libpikmin_assets::{AnimSet, Assets, Skeleton, paths};

fn main() {
    let a = Assets::discover(None).unwrap();
    let m = a.model(paths::ONION).unwrap();
    let skel = Skeleton::new(&m);
    let set = AnimSet::load(&a, paths::ONION_ANIM_MGR, paths::ONION_BUNDLE).unwrap();
    let (_, takeoff) = set.get(7).unwrap(); // c_red_t, red take-off
    let (_, idle) = set.get(1).unwrap(); // flowwait, the landed Rest the view rebases against
    let rest = skel.pose(Mat4::IDENTITY, Some((idle, 0.0)));
    for step in 0..=12 {
        let frame = 200.0 + step as f32 * 5.0;
        let pose = skel.pose(Mat4::IDENTITY, Some((takeoff, frame)));
        let offset = rest.joints[1].w_axis.truncate() - pose.joints[1].w_axis.truncate();
        print!("frame {frame:>5.0}:");
        for (leg, joints) in [[3usize, 4, 5], [6, 7, 8], [9, 10, 11]].into_iter().enumerate() {
            print!(" leg {leg}");
            for j in joints {
                let p = pose.joints[j].w_axis.truncate() + offset;
                print!(" ({:.1}, {:.1}, {:.1})", p.x, p.y, p.z);
            }
            print!(" |");
        }
        println!();
    }
}
