//! Host intro music, rendered from the extracted disc banks on first run without a device.
//! Mirrors Sunshine's `ror2_audio` cache: no music files ship beside the plugin.

use std::{path::Path, sync::Arc};

use libpikmin_assets::{
    Assets,
    sound::{SoundData, tables::JaudioTables},
};
use libpikmin_audio::SoundSystem;

pub const STINGS_VERSION: &str = "1";
const RATE: u32 = 32_000;

pub fn render_stings(dir: &Path) -> Result<(), String> {
    render_into(dir, &dir.join("ror2_audio"))
}

fn render_into(dir: &Path, out: &Path) -> Result<(), String> {
    let path = out.join("spawn.wav");
    let marker = out.join("stings.version");
    if path.is_file() && std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == STINGS_VERSION)
    {
        return Ok(());
    }
    let assets = Assets {
        root: dir.to_owned(),
    };
    let data = Arc::new(SoundData::load(&assets).map_err(|e| e.to_string())?);
    let tables = JaudioTables::load(&assets).map_err(|e| e.to_string())?;
    let mut sys = SoundSystem::new(data, tables);
    // piki_scene.c: Impact Site is BGM_Tutorial (4, tutorial.jam), as in the old build tool.
    sys.play_bgm(0, 4);
    let mut pcm = vec![0.0; RATE as usize * 7]; // 3.5 seconds, stereo.
    sys.render(&mut pcm, RATE as f32);
    let length = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + length as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + length).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&[1, 0, 2, 0]);
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 4).to_le_bytes());
    wav.extend_from_slice(&[4, 0, 16, 0]);
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&length.to_le_bytes());
    for sample in pcm {
        wav.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let temp = path.with_extension("wav.tmp");
    std::fs::write(&temp, wav).map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())?;
    std::fs::write(marker, STINGS_VERSION).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intro_is_audible_stereo_and_reused_without_disc_banks() {
        let Some(assets) = Assets::discover(None) else {
            return;
        };
        let out = libcommon::scratch::dir(env!("CARGO_MANIFEST_DIR"), "pikmin-intro");
        render_into(&assets.root, &out).unwrap();
        let path = out.join("spawn.wav");
        let wav = std::fs::read(&path).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), RATE);
        assert_eq!(wav.len(), 44 + RATE as usize * 7 * 2);
        for channel in 0..2 {
            assert!(
                wav[44..].chunks_exact(4).any(|frame| {
                    i16::from_le_bytes(frame[channel * 2..channel * 2 + 2].try_into().unwrap())
                        .unsigned_abs()
                        > 100
                }),
                "channel {channel} is silent"
            );
        }
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        let missing = out.join("no-banks");
        render_into(&missing, &out).expect("a complete cache never reloads banks");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        std::fs::write(out.join("stings.version"), "old").unwrap();
        assert!(
            render_into(&missing, &out).is_err(),
            "stale cache must render again"
        );
        render_into(&assets.root, &out).unwrap();
        std::fs::remove_file(&path).unwrap();
        render_into(&assets.root, &out).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            wav,
            "missing clip is repaired deterministically"
        );
        std::fs::remove_dir_all(out).unwrap();
    }
}
