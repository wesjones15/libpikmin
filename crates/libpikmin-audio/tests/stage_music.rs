use std::sync::Arc;
use libpikmin_assets::{Assets, sound::{SoundData, tables::JaudioTables}};
use libpikmin_audio::SoundSystem;

#[test]
fn disc_stage_songs_render_and_enemy_proximity_changes_the_original_mix() {
    let Some(assets) = Assets::discover(None) else { return };
    let data = Arc::new(SoundData::load(&assets).unwrap());
    let tables = JaudioTables::load(&assets).unwrap();
    let mut total_difference = 0.0f64;
    for song in [4, 5, 10, 9, 17] {
        let mut quiet = SoundSystem::new(data.clone(), tables.clone());
        let mut nearby = SoundSystem::new(data.clone(), tables.clone());
        quiet.play_bgm(0, song); nearby.play_bgm(0, song);
        let mut peak = 0.0f32;
        for _ in 0..120 {
            quiet.frame(); nearby.frame();
            // `Jac_UpdateCamera` (pikiinter.c): nearby event-type-1 enemies drive flag 1.
            quiet.set_bgm_mode_flag(0, 1, 0); nearby.set_bgm_mode_flag(0, 1, 1);
            let a = quiet.render_native(1.0 / 30.0);
            let b = nearby.render_native(1.0 / 30.0);
            for (&left, &right) in a.iter().zip(&b) {
                assert!(left.is_finite() && right.is_finite());
                peak = peak.max(left.abs()).max(right.abs());
                total_difference += (left - right).abs() as f64;
            }
        }
        assert!(peak > 0.001, "stage BGM {song} must produce real disc music");
    }
    assert!(total_difference > 1.0, "enemy proximity must change the musical layers");
}

#[test]
fn ship_repair_menu_songs_render_from_the_disc() {
    let Some(assets) = Assets::discover(None) else { return };
    let data = Arc::new(SoundData::load(&assets).unwrap());
    let tables = JaudioTables::load(&assets).unwrap();
    // piki_scene.c: title jungle and world-map (space/night) menu music.
    for song in [7, 12] {
        let mut audio = SoundSystem::new(data.clone(), tables.clone());
        audio.play_bgm(0, song);
        let mut peak = 0.0f32;
        for _ in 0..90 {
            audio.frame();
            for &sample in &audio.render_native(1.0 / 30.0) {
                assert!(sample.is_finite());
                peak = peak.max(sample.abs());
            }
        }
        assert!(peak > 0.001, "menu BGM {song} must render original music");
    }
}
