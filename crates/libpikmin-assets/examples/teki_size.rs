//! Bind-pose bounds of teki models at their disc scale.
use glam::Mat4;
use libpikmin_assets::teki::{TekiParams, teki_paths, tpf};
use libpikmin_assets::{Assets, Skeleton};
fn main() {
    let a = Assets::discover(None).unwrap();
    for (name, ints) in [("chappy", 21), ("swallow", 20), ("qurione", 20)] {
        let [m, ..] = teki_paths(name);
        let model = a.model(&m).unwrap();
        let p = TekiParams::parse_any(&a.read(&teki_paths(name)[3]).unwrap(), ints).unwrap();
        let s = p.f(tpf::SCALE);
        let pose = Skeleton::new(&model).pose(Mat4::from_scale(glam::Vec3::splat(s)), None);
        let (mut lo, mut hi) = (glam::Vec3::splat(1e9), glam::Vec3::splat(-1e9));
        for b in model.batches().unwrap() { for v in &b.vertices { let q = pose.skin(v).0; lo = lo.min(q); hi = hi.max(q); } }
        println!("{name} scale {s}: {lo} .. {hi}; flight {} life gauge {} corpse {} {} attack hit {}", p.f(tpf::FLIGHT_HEIGHT), p.f(tpf::LIFE_GAUGE_OFFSET), p.f(tpf::CORPSE_SIZE), p.f(tpf::CORPSE_HEIGHT), p.f(tpf::ATTACK_HIT_RANGE));
        for (i, j) in pose.joints.iter().enumerate() { let t = j.w_axis.truncate(); print!("{i}:{:.0},{:.0},{:.0} ", t.x, t.y, t.z); }
        println!();
    }
}
