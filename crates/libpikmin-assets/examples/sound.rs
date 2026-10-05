//! Lists the sound bank and sequences; with an output folder, also writes some waves and the
//! streams as `.wav` files. `cargo run -p libpikmin-assets --example sound [out_dir]`

use libpikmin_assets::Assets;
use libpikmin_assets::sound::bank::Program;
use libpikmin_assets::sound::{SoundData, Stream};

fn write_wav(path: &std::path::Path, rate: u32, channels: u16, samples: &[i16]) {
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    let data = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

fn main() {
    let assets = Assets::discover(None).expect("no assets folder");
    let snd = SoundData::load(&assets).unwrap();
    let out = std::env::args().nth(1).map(std::path::PathBuf::from);

    for (i, w) in snd.bank.wsys.iter().enumerate() {
        let Some(w) = w else { continue };
        let waves: usize = w.archives.iter().map(|a| a.waves.len()).sum();
        let formats: std::collections::BTreeSet<u8> = w
            .archives
            .iter()
            .flat_map(|a| a.waves.iter().map(|w| w.format))
            .collect();
        println!(
            "wsys {i:2} virt {:#x}: {} archives {:?}, {waves} waves, formats {formats:?}, {} scenes (ext {:?}, deps {:?})",
            w.virtual_id,
            w.archives.len(),
            w.archives
                .iter()
                .map(|a| a.file.as_str())
                .collect::<Vec<_>>(),
            w.scenes.len(),
            w.scenes.iter().map(|s| s.external).collect::<Vec<_>>(),
            w.scenes
                .iter()
                .map(|s| s.dependencies.clone())
                .collect::<Vec<_>>(),
        );
        for (si, s) in w.scenes.iter().enumerate() {
            println!(
                "   scene {si}: cdf {} cex {} cst {}",
                s.cdf.len(),
                s.cex.len(),
                s.cst.len()
            );
        }
    }
    for (i, b) in snd.bank.banks.iter().enumerate() {
        let Some(b) = b else { continue };
        let insts = b
            .programs
            .iter()
            .filter(|p| matches!(p, Some(Program::Inst(_))))
            .count();
        let percs = b
            .programs
            .iter()
            .filter(|p| matches!(p, Some(Program::Perc(_))))
            .count();
        println!(
            "bank {i:2} virt {:#x}: {insts} insts, {percs} percs",
            b.virtual_id
        );
    }
    for i in 0..snd.seqs.len() {
        if let Some(e) = &snd.seqs.entries[i] {
            println!("seq {i:2}: {:14} {:#8x} {:#7x}", e.name, e.offset, e.size);
        }
    }

    let Some(out) = out else { return };
    std::fs::create_dir_all(&out).unwrap();
    // The first few waves of the SE wave system.
    for (wsi, ai) in [(0usize, 1usize), (5, 0), (0, 0)] {
        let Some(w) = snd.bank.wsys[wsi].as_ref() else {
            continue;
        };
        for (wi, wave) in w.archives[ai].waves.iter().enumerate().take(8) {
            let pcm = snd.pcm(wsi, ai, wi).unwrap();
            let rms = (pcm.iter().map(|&s| (s as f64).powi(2)).sum::<f64>()
                / pcm.len().max(1) as f64)
                .sqrt();
            let diff = (pcm
                .windows(2)
                .map(|w| ((w[1] as f64) - (w[0] as f64)).powi(2))
                .sum::<f64>()
                / pcm.len().max(1) as f64)
                .sqrt();
            let clip = pcm
                .iter()
                .filter(|&&s| s == i16::MAX || s == i16::MIN)
                .count();
            println!(
                "wave {wi}: fmt {} {}B key {} rate {} loop {} {}..{} -> {} samples, rms {rms:.0} diff {diff:.0} clip {clip}",
                wave.format,
                wave.length,
                wave.key,
                wave.sample_rate,
                wave.looping,
                wave.loop_start,
                wave.end,
                pcm.len()
            );
            write_wav(
                &out.join(format!("w{wsi}_{ai}_{wi:03}.wav")),
                wave.sample_rate as u32,
                1,
                &pcm,
            );
        }
    }
    for name in ["piki.stx", "onion.stx"] {
        let s = Stream::decode(&assets.read(&format!("SndData/{name}")).unwrap()).unwrap();
        println!("{name}: {:?}", s.header);
        write_wav(
            &out.join(name.replace(".stx", ".wav")),
            s.header.sample_rate as u32,
            2,
            &s.stereo,
        );
    }
}
