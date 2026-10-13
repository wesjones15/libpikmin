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

/// Peak sample and energy of the trumpet playing for a while at `gain`.
fn trumpet(gain: f32) -> (f32, f32) {
    let assets = Assets::discover(None).expect("assets");
    let data = Arc::new(SoundData::load(&assets).unwrap());
    let mut sound = SoundSystem::new(data, JaudioTables::load(&assets).unwrap());
    sound.set_formation_gain(gain);
    let (mut peak, mut energy) = (0f32, 0f32);
    for frame in 0..150 {
        sound.formation(1., 0.);
        sound.frame();
        let pcm = sound.render_native(1. / 30.);
        if frame > 60 {
            peak = pcm.iter().fold(peak, |p, s| p.max(s.abs()));
            energy += pcm.iter().map(|s| s * s).sum::<f32>();
        }
    }
    (peak, energy)
}

#[test]
fn trumpet_gain_makes_the_swarm_louder_without_clipping() {
    if Assets::discover(None).is_none() {
        return;
    }
    let (disc_peak, disc) = trumpet(1.0);
    let (loud_peak, loud) = trumpet(1.4);
    let (quiet_peak, quiet) = trumpet(0.5);
    assert!(disc > 0.01);
    let ratio = (loud / disc).sqrt();
    assert!((1.25..1.55).contains(&ratio), "amplitude x{ratio} for gain 1.4");
    assert!(quiet < disc && (quiet / disc).sqrt() < 0.6, "gain below 1 must turn it down");
    assert!(loud_peak < 1.0, "peak {loud_peak} (disc {disc_peak}, quiet {quiet_peak}) would clip");
    // The disc's level is untouched at gain 1.
    let (again, _) = trumpet(1.0);
    assert_eq!(again, disc_peak);
}
