//! Prints a particle effect's authored colour keys and blend.
//! `cargo run -p libpikmin-assets --example effect_dump -- effects/pcr/take_s.pcr`
use libpikmin_assets::{Assets, particle::Pcr};

fn main() {
    let a = Assets::discover(None).unwrap();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(p) = args.next() {
        if p == "--tex" {
            let Some(path) = args.next() else { break };
            let d = a.read(&path).unwrap();
            let (img, _, _) = libpikmin_assets::texture::decode_bti(&d).unwrap();
            let n = (img.rgba.len() / 4).max(1) as f64;
            let mean = |k: usize| img.rgba.iter().skip(k).step_by(4).map(|&v| v as f64).sum::<f64>() / n;
            println!("{path}: {}x{} mean rgba {:.0} {:.0} {:.0} {:.0}", img.width, img.height,
                mean(0), mean(1), mean(2), mean(3));
            continue;
        }
        let d = match a.read(&p) { Ok(d) => d, Err(e) => { println!("{p}: {e}"); continue } };
        let pcr = Pcr::parse(&d).unwrap();
        println!(
            "{p}: billboard {} colour mode {} blend {:#x} z {:#x} max_passes {} max_frame {} life {} size {} rate {:?}",
            pcr.billboard, pcr.color_mode(), pcr.blend_factor, pcr.z_mode,
            pcr.max_passes, pcr.max_frame, pcr.life, pcr.size, pcr.rate
        );
        println!("  anim duration {} flags {} thresholds {:?}", pcr.anim.duration, pcr.anim.flags, pcr.anim.thresholds);
        println!("  scale thresholds {:?} min {:?} alpha thresholds {:?} size jitter {} life jitter {}",
            pcr.scale_thresholds, pcr.min_scales, pcr.alpha_thresholds, pcr.size_jitter, pcr.life_jitter);
        println!("  prim {:?}", pcr.anim.prim);
        println!("  env  {:?}", pcr.anim.env);
        println!("  children {:?}", pcr.children);
    }
}
