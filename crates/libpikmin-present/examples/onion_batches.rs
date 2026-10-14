//! Lists the Onion model's batches: material, blend, cull and extent, to check how they layer.
//! `cargo run -p libpikmin-present --example onion_batches`
use libpikmin_assets::Assets;
use libpikmin_present::view::{models, View};

fn main() {
    let assets = Assets::discover(None).expect("no assets");
    let view = View::load(&assets).unwrap();
    let vm = &view.models[models::ONION as usize];
    for (i, b) in vm.batches.iter().enumerate() {
        let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
        for v in &b.vertices {
            lo = lo.min(v.position);
            hi = hi.max(v.position);
        }
        let m = &vm.model.materials[b.material];
        println!("  pe {:x?} flags {:#x}", m.pe, m.flags);
        println!("batch {i}: material {} blend {} cull {:?} tris {} tex {} y {:.0}..{:.0} x {:.0}..{:.0} z {:.0}..{:.0}",
            b.material, vm.batch_blend[i], b.cull, b.indices.len() / 3, vm.batch_texture[i], lo.y, hi.y, lo.x, hi.x, lo.z, hi.z);
    }
}
