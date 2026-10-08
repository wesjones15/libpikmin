use std::sync::Arc;
use libpikmin_assets::{Assets, sound::{SoundData, tables::JaudioTables}};
use libpikmin_audio::SoundSystem;

#[test]
fn disc_formation_trumpet_starts_and_stops_without_music() {
    let Some(assets) = Assets::discover(None) else { return; };
    let data = Arc::new(SoundData::load(&assets).unwrap());
    let mut sound = SoundSystem::new(data, JaudioTables::load(&assets).unwrap());
    let mut energy = |active: bool| {
        let mut energy = 0.;
        for frame in 0..90 {
            sound.formation(if active { 1. } else { 0. }, 0.);
            sound.frame();
            let pcm = sound.render_native(1. / 30.);
            if frame > 60 { energy += pcm.iter().map(|s|s*s).sum::<f32>(); }
        }
        energy
    };
    let silent = energy(false);
    let playing = energy(true);
    let stopped = energy(false);
    assert!(playing > silent + 0.01, "formation must produce sound: {playing} vs {silent}");
    assert!(stopped < playing * 0.01, "trumpet must stop on release: {stopped} vs {playing}");
}
