//! Renders the game's music and sound effects to `.wav` files.
//!
//! `cargo run -p libpikmin-audio --example render -- <out_dir> [bgm id] [seconds]`
//! writes `bgm_<id>.wav`, then one file per sound in `SOUNDS`.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use libpikmin_assets::Assets;
use libpikmin_assets::sound::SoundData;
use libpikmin_assets::sound::tables::JaudioTables;
use libpikmin_audio::SoundSystem;
use libpikmin_audio::calc::DAC_RATE;

const SOUNDS: &[&str] = &[
    "SE_GATHER",
    "SE_THROW",
    "SE_PIKI_PULLED",
    "SE_PIKI_CALLED",
    "SE_PIKI_FLY",
    "SE_PIKI_PULLING",
    "SE_CHAPPY_WALK",
    "SE_CHAPPY_BITE",
    "SE_PIKI_ATTACK_VOICE",
    "SYSSE_DECIDE1",
    "SYSSE_CMENU_ON",
    "SE_LIFT_TRY",
    "SE_LIFT_MOVE",
    "SEF_PIKI_HANG",
    "SE_CONTAINER_PIKIBORN",
    "SE_PIKI_GROW0",
    "SE_PIKI_GROW1",
    "SE_PIKI_GROW2",
    "SE_PIKI_GROW5",
    "SE_PIKI_DEAD",
];

fn write_wav(path: &std::path::Path, rate: u32, samples: &[f32]) {
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    let data = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

fn stats(name: &str, v: &[f32]) {
    let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len().max(1) as f32).sqrt();
    let peak = v.iter().fold(0f32, |m, s| m.max(s.abs()));
    let clip = v.iter().filter(|s| s.abs() >= 1.0).count();
    // Loudness per half second.
    let win = (DAC_RATE as usize / 2) * 2;
    let env: Vec<String> = v
        .chunks(win)
        .map(|c| {
            format!(
                "{:.3}",
                (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()
            )
        })
        .collect();
    println!(
        "{name}: rms {rms:.4} peak {peak:.3} clipped {clip}\n  {}",
        env.join(" ")
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    let bgm: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(5);
    let secs: f32 = args.next().map(|s| s.parse().unwrap()).unwrap_or(20.0);
    std::fs::create_dir_all(&out).unwrap();
    let assets = Assets::discover(None).expect("assets");
    let data = Arc::new(SoundData::load(&assets).unwrap());
    let tables = JaudioTables::load(&assets).unwrap();

    let mut s = SoundSystem::new(data.clone(), tables.clone());
    for id in [
        0x10000, 0x10003, 0x10007, 0x10008, 0x10009, 0x1000A, 0x1000F, 0x20000, 0x30000,
    ] {
        println!("track {id:#x}: {}", s.has_track(id));
    }
    if bgm != 0 {
        s.play_bgm(0, bgm);
        let v = s.render_native(secs);
        stats(
            &format!("bgm {bgm} ({})", data.seqs.name(bgm).unwrap_or("?")),
            &v,
        );
        write_wav(&out.join(format!("bgm_{bgm}.wav")), DAC_RATE as u32, &v);
    }

    for name in SOUNDS {
        let mut s = SoundSystem::new(data.clone(), tables.clone());
        s.set_listener(Mat4::IDENTITY, Vec3::ZERO);
        s.play(name, Vec3::new(50.0, 0.0, 0.0));
        let mut v = Vec::new();
        for _ in 0..60 {
            s.frame();
            v.extend(s.render_native(1.0 / 30.0));
        }
        if *name == "SE_GATHER" {
            s.stop(name);
            for _ in 0..30 {
                s.frame();
                v.extend(s.render_native(1.0 / 30.0));
            }
        }
        stats(name, &v);
        write_wav(&out.join(format!("{name}.wav")), DAC_RATE as u32, &v);
    }
}
