//! A separate optional music stream, so host music/SFX settings stay independent.
use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use libpikmin_assets::Assets;
use libpikmin_present::Audio;

pub struct PikMusic { audio: Audio, p2: Option<libpikmin_present::p2music::Music>, p2_dir: std::path::PathBuf, poisoned: bool }

/// # Safety
/// `folder` is a valid C string (or null to discover assets).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_new(folder: *const c_char) -> *mut PikMusic {
    catch_unwind(AssertUnwindSafe(|| {
        let folder = unsafe { super::path_arg(folder) };
        let assets = Assets::discover(folder.as_deref())?;
        let audio = Audio::start(&assets).ok()?;
        audio.set_volume(0.0);
        if !libpikmin_present::audio::STAGE_BGM.iter().all(|&id| audio.has_bgm(id)) ||
            ![7, 12].iter().all(|&id| audio.has_bgm(id)) { return None; }
        Some(Box::into_raw(Box::new(PikMusic { audio, p2: None, p2_dir: assets.root.join("p2"), poisoned: false })))
    })).ok().flatten().unwrap_or(std::ptr::null_mut())
}

/// BGM IDs: 4/5/10/9/17 stages, 7 title jungle, 12 world-map night menu.
/// Zero stops the song. No boss override here.
/// # Safety
/// `music` is a live handle; calls are serialized by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_update(music: *mut PikMusic, song: u32, enemy: u8, volume: f32, paused: u8) -> u32 {
    unsafe {pik_music_update_context(music,song,if enemy!=0 {1}else{0},volume,paused,1.0/30.0)}
}

/// Pikmin system menu cues: open, select, and confirm.
/// # Safety
/// `music` is a live handle; calls are serialized by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_menu_sound(music: *mut PikMusic, cue: u32) {
    let Some(m) = (unsafe { music.as_mut() }) else { return };
    if m.poisoned { return; }
    let name = match cue {
        0 => "SYSSE_CMENU_ON",
        1 => "SYSSE_CMENU_SELECT",
        2 => "SYSSE_SELECT_DECIDE",
        _ => return,
    };
    if catch_unwind(AssertUnwindSafe(|| m.audio.play(name))).is_err() { m.poisoned = true; }
}

/// # Safety
/// `music` came from `pik_music_new` and is not used again. Null is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_free(music: *mut PikMusic) {
    if !music.is_null() { let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(music) }))); }
}

/// Stage context: bits 0 enemy near, 1 battle, 2 working, 3 treasure, 4 squad.
/// P2 songs 0x100..0x103 name p2music::SONGS; all old P1 ids stay supported.
/// # Safety
/// Same as pik_music_update; serialized calls on a live music handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pik_music_update_context(music:*mut PikMusic,song:u32,flags:u32,volume:f32,paused:u8,dt:f32)->u32 {
    let Some(m)=(unsafe {music.as_mut()}) else {return 0};
    if m.poisoned || (song!=0 && !(0x100..0x104).contains(&song) && !libpikmin_present::audio::STAGE_BGM.contains(&(song as usize)) && song!=7 && song!=12) {return 0;}
    let dt=if dt.is_finite(){dt.clamp(0.0,0.25)}else{0.0};
    match catch_unwind(AssertUnwindSafe(|| {
        if (0x100..0x104).contains(&song) {
            if m.p2.is_none(){m.p2=libpikmin_present::p2music::Music::open(&m.p2_dir);}
            let Some(p2)=m.p2.as_mut() else {return 0};
            if !p2.update((song-0x100) as usize,flags,volume,paused!=0,dt) {return 0;}
            m.audio.stop_bgm();m.audio.set_volume(0.0);return 1;
        }
        if let Some(p2)=m.p2.as_mut(){p2.stop();}
        if m.audio.bgm()!=song as usize {if song==0 {m.audio.stop_bgm();}else{m.audio.play_bgm(song as usize);}}
        m.audio.set_volume(if volume.is_finite(){volume}else{0.0});
        m.audio.music_frame(flags&1!=0,paused!=0);1
    })) {Ok(ok)=>ok,Err(_)=>{m.poisoned=true;0}}
}
