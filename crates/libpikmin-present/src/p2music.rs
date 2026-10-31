//! Pikmin 2 directed stage music on the existing JAudio output. The disc's track maps
//! choose the layers; a host supplies enemy/battle/work/treasure/squad activity.
use libsms_assets::sound::p2music::{SONGS, TrackMap};
use libsms_present::audio::Pikmin2Audio;
use std::{collections::HashMap, path::Path};
pub struct Music {
    audio: Pikmin2Audio,
    maps: HashMap<String, TrackMap>,
    song: Option<usize>,
    levels: [f32; 16],
}
impl Music {
    pub fn open(dir: &Path) -> Option<Self> {
        let maps = libsms_assets::sound::p2music::load(dir).ok()?;
        if !SONGS.iter().all(|s| maps.contains_key(*s)) {
            return None;
        }
        let audio = Pikmin2Audio::start(dir)?;
        audio.set_volume(0.0);
        Some(Self {
            audio,
            maps,
            song: None,
            levels: [0.0; 16],
        })
    }
    pub fn update(&mut self, song: usize, flags: u32, volume: f32, paused: bool, dt: f32) -> bool {
        let Some(&name) = SONGS.get(song) else {
            return false;
        };
        if self.song != Some(song) {
            if !self.audio.play_music(name) {
                return false;
            }
            self.song = Some(song);
            self.levels = [0.0; 16];
        }
        let target = self.maps[name].levels(flags);
        if !paused {
            for i in 0..16 {
                let d = target[i] - self.levels[i];
                self.levels[i] += d.clamp(-dt * 3.0, dt * 3.0);
            }
        }
        self.audio
            .set_volume(if volume.is_finite() { volume } else { 0.0 });
        self.audio.set_paused(paused);
        self.audio.music_layers(&self.levels);
        if !paused {
            self.audio.advance(dt);
        }
        true
    }
    pub fn stop(&mut self) {
        self.audio.stop_music();
        self.audio.set_volume(0.0);
        self.song = None;
    }
}
