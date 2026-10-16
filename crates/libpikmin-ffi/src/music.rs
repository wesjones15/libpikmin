//! A separate optional music stream, so host music/SFX settings stay independent.
use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use libpikmin_assets::Assets;
use libpikmin_present::Audio;

pub struct PikMusic { audio: Audio, poisoned: bool }

/// # Safety
/// `folder` is a valid C string (or null to discover assets).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_new(folder: *const c_char) -> *mut PikMusic {
    catch_unwind(AssertUnwindSafe(|| {
        let folder = unsafe { super::path_arg(folder) };
        let assets = Assets::discover(folder.as_deref())?;
        let audio = Audio::start(&assets).ok()?;
        audio.set_volume(0.0);
        if !libpikmin_present::audio::STAGE_BGM.iter().all(|&id| audio.has_bgm(id)) { return None; }
        Some(Box::into_raw(Box::new(PikMusic { audio, poisoned: false })))
    })).ok().flatten().unwrap_or(std::ptr::null_mut())
}

/// Stage BGM IDs: 4 Impact Site, 5 Forest of Hope, 10 Forest Navel,
/// 9 Distant Spring, 17 Final Trial. Zero stops the song. No boss override here.
/// # Safety
/// `music` is a live handle; calls are serialized by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_update(music: *mut PikMusic, song: u32, enemy: u8, volume: f32, paused: u8) -> u32 {
    let Some(m) = (unsafe { music.as_mut() }) else { return 0 };
    if m.poisoned || (song != 0 && !libpikmin_present::audio::STAGE_BGM.contains(&(song as usize))) { return 0; }
    match catch_unwind(AssertUnwindSafe(|| {
        if m.audio.bgm() != song as usize {
            if song == 0 { m.audio.stop_bgm(); } else { m.audio.play_bgm(song as usize); }
        }
        m.audio.set_volume(if volume.is_finite() { volume } else { 0.0 });
        m.audio.music_frame(enemy != 0, paused != 0);
    })) { Ok(()) => 1, Err(_) => { m.poisoned = true; 0 } }
}

/// # Safety
/// `music` came from `pik_music_new` and is not used again. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_free(music: *mut PikMusic) {
    if !music.is_null() { let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(music) }))); }
}
