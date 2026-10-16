//! Audit the user's ship-mode assets and export an optional local victory audition.
//! No extracted content belongs in Git. `cargo run -p libpikmin-assets --example ship_mode_audit -- assets/ship-mode-audit`
use libpikmin_assets::{Assets, AnimSet, motion::Motion, ship, sound::Stream};

fn main() {
    let assets = Assets::discover(None).expect("extract your disc first");
    let infos = ship::parse_pel_anim(&assets.read(ship::PEL_ANIM).unwrap()).unwrap();
    for (index, id) in ship::PART_IDS.iter().enumerate() {
        let info = infos.iter().find(|p| p.id == *id).expect("part animation descriptor");
        let model = assets.model(&info.model()).expect("part model");
        let anims = AnimSet::load(&assets, &info.anim_mgr(), &info.bundle()).expect("part animations");
        assert!(anims.get(0).is_some(), "missing carry motion {index}");
        println!("part {index:02} {id}: {} joints, carry animation ready", model.joints.len());
    }
    let pikis = AnimSet::load(&assets, "pikis/animMgr.bin", "pikis/bluModel.anm").unwrap();
    for motion in [Motion::RotJump, Motion::WaveJmp] {
        let (_, clip) = pikis.get(motion.anim_index().unwrap()).expect("celebration clip");
        println!("celebration {motion:?}: {} frames", clip.frames);
    }
    let stream = Stream::decode(&assets.read("SndData/compend0.stx").unwrap()).unwrap();
    println!("complete-ending farewell: {} Hz, {:.2} s", stream.header.sample_rate, stream.header.samples as f64 / stream.header.sample_rate as f64);
    if let Some(out) = std::env::args_os().nth(1) {
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        let rate = stream.header.sample_rate as u32;
        let size = stream.stereo.len() as u32 * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF"); wav.extend_from_slice(&(size + 36).to_le_bytes()); wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); wav.extend_from_slice(&1u16.to_le_bytes()); wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes()); wav.extend_from_slice(&(rate * 4).to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes()); wav.extend_from_slice(&16u16.to_le_bytes()); wav.extend_from_slice(b"data"); wav.extend_from_slice(&size.to_le_bytes());
        for sample in stream.stereo { wav.extend_from_slice(&sample.to_le_bytes()); }
        std::fs::write(out.join("victory-compend0.wav"), wav).unwrap();
    }
}
