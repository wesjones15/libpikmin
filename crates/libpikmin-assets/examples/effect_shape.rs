//! Prints an effect model's size, materials and colour animation (`EffectMgr`'s geometry
//! effects, like the Onions' light cones).
//! `cargo run -p libpikmin-assets --example effect_shape -- effects/shapes/r_goal.mod`
use libpikmin_assets::{Assets, Model};

fn main() {
    let a = Assets::discover(None).unwrap();
    for p in std::env::args().skip(1) {
        let m = Model::parse(&a.read(&p).unwrap()).unwrap();
        let batches = m.batches().unwrap();
        let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
        for v in batches.iter().flat_map(|b| &b.vertices) {
            lo = lo.min(v.position);
            hi = hi.max(v.position);
        }
        println!("{p}: {} batches, joints {}, bounds {lo} .. {hi}", batches.len(), m.joints.len());
        for (i, b) in batches.iter().enumerate() {
            println!("  batch {i}: material {} verts {}", b.material, b.vertices.len());
        }
        for (i, mat) in m.materials.iter().enumerate() {
            println!(
                "  mat {i} flags {:#x} blend {:?} color {:?} light {:#x} anim frames {} speed {} alpha keys {:?}",
                mat.flags, mat.blend(), mat.color, mat.lighting, mat.color_anim.frames, mat.color_anim.speed,
                mat.color_anim.alpha.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>()
            );
            println!("    pe {:x?} additive {}", mat.pe, mat.additive());
            println!("    gens {:?}", mat.tex_gens);
        }
    }
}
