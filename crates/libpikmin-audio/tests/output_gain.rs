use std::sync::Arc;
use libpikmin_assets::{Assets, sound::{SoundData, tables::JaudioTables}};
use libpikmin_audio::SoundSystem;

#[test]
fn host_attenuation_preserves_peaks_before_saturation() {
    let Some(assets) = Assets::discover(None) else {
        eprintln!("SKIP: local Pikmin assets required");
        return;
    };
    let data = Arc::new(SoundData::load(&assets).unwrap());
    for rate in [32000, 48000] {
        let mut normal = SoundSystem::new(data.clone(), JaudioTables::load(&assets).unwrap());
        let mut hot = SoundSystem::new(data.clone(), JaudioTables::load(&assets).unwrap());
        hot.engine.dsp_level *= 8.0;
        let mut a = vec![0.0f32; rate / 30 * 2];
        let mut b = a.clone();
        let mut peak = 0.0f32;
        let mut error = 0.0f32;
        for _ in 0..120 {
            normal.formation(1.0, 0.0);
            hot.formation(1.0, 0.0);
            normal.frame();
            hot.frame();
            normal.render(&mut a, rate as f32);
            hot.render_with_gain(&mut b, rate as f32, 0.125);
            for (&x, &y) in a.iter().zip(&b) {
                assert!(x.is_finite() && y.is_finite());
                peak = peak.max(x.abs());
                error = error.max((x-y).abs());
            }
        }
        eprintln!("{rate} Hz: peak {peak}, waveform error {error}");
        assert!(peak > 0.125, "fixture must exceed the old clip-before-gain ceiling");
        assert!(error < 0.00001, "host attenuation must preserve the waveform, not flatten it");
        hot.render_with_gain(&mut b, rate as f32, 0.0);
        assert!(b.iter().all(|&s| s == 0.0), "mute must include buffered samples");
    }
}
