//! Writes a view model's baked batch textures as PPM files (colour, then alpha as grey), to
//! check what hosts will draw. `cargo run -p libpikmin-present --example baked -- 17 out_dir`
use libpikmin_assets::Assets;
use libpikmin_present::view::View;

fn main() {
    let mut args = std::env::args().skip(1);
    let model: usize = args.next().expect("model index").parse().unwrap();
    let out = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
    let assets = Assets::discover(None).expect("no assets");
    let view = View::load(&assets).unwrap();
    let vm = &view.models[model];
    for (b, &id) in vm.batch_texture.iter().enumerate() {
        println!("batch {b}: texture {id} blend {} light {}", vm.batch_blend[b], vm.batch_light(b));
        let Ok(i) = usize::try_from(id) else { continue };
        let img = &view.textures.list[i];
        let (mut sum, n) = ([0u64; 4], (img.width * img.height) as u64);
        for px in img.rgba.chunks(4) {
            for c in 0..4 {
                sum[c] += px[c] as u64;
            }
        }
        println!("  {}x{} mean rgba {:?}", img.width, img.height, sum.map(|s| s / n.max(1)));
        for (suffix, alpha) in [("rgb", false), ("a", true)] {
            let mut ppm = format!("P6 {} {} 255\n", img.width, img.height).into_bytes();
            for px in img.rgba.chunks(4) {
                if alpha { ppm.extend([px[3]; 3]) } else { ppm.extend(&px[..3]) }
            }
            std::fs::write(out.join(format!("model{model}_batch{b}_{suffix}.ppm")), ppm).unwrap();
        }
    }
}
