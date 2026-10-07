//! The viewer's sound: `libpikmin-present`'s [`Inner`] (the sound system on a cpal stream and
//! the sim's events mapped to the game's sounds), fed the viewer's own enemies.

use glam::{Mat4, Vec3};
use libpikmin::Sim;
use libpikmin::event::Event;
use libpikmin_assets::Assets;
use libpikmin_present::audio::{Audio as Inner, Creatures};
pub use libpikmin_present::audio::{STAGE_BGM, SceneSounds};

use crate::teki::Tekis;

pub struct Audio(Inner);

fn positions(tekis: &Tekis) -> Vec<(libpikmin::CreatureId, Vec3)> {
    tekis.list.iter().map(|t| (t.id, t.pos)).collect()
}

impl Audio {
    /// Starts the sound system and an output stream. `None` if there's no sound data or no
    /// output device.
    pub fn start(assets: &Assets) -> Option<Audio> {
        match Inner::start(assets) {
            Ok(a) => {
                println!("sound: {} Hz, {} channels", a.rate, a.channels);
                Some(Audio(a))
            }
            Err(e) => {
                eprintln!("sound: {e}");
                None
            }
        }
    }

    pub fn play_bgm(&mut self, id: usize) {
        self.0.play_bgm(id);
    }

    pub fn bgm(&self) -> usize {
        self.0.bgm()
    }

    pub fn stop_bgm(&mut self) {
        self.0.stop_bgm();
    }

    /// A system (menu) sound by name.
    pub fn play(&self, name: &str) {
        self.0.play(name);
    }

    /// A command for the cutscene sound track (`Jac_DemoSound`), or `None` to end it.
    pub fn demo(&self, cmd: Option<(u16, u16)>) {
        self.0.demo(cmd);
    }

    /// Once per game frame: the camera, emitter positions, enemy animation sounds, and
    /// whether the active captain is whistling.
    pub fn frame(&mut self, view: Mat4, listener: Vec3, sim: &Sim, tekis: &mut Tekis, whistling: bool, scene: SceneSounds) {
        let table_sounds: Vec<_> = tekis
            .list
            .iter_mut()
            .flat_map(|t| {
                let (id, base) = (t.id, t.kind.sound_base());
                std::mem::take(&mut t.sounds).into_iter().map(move |i| (id, base, i))
            })
            .collect();
        let positions = positions(tekis);
        let creatures = Creatures { positions: &positions, table_sounds: &table_sounds };
        self.0.frame(view, listener, sim, creatures, whistling, scene);
    }

    /// The sounds the game plays for a sim event.
    pub fn on_event(&mut self, e: &Event, sim: &Sim, tekis: &Tekis) {
        let positions = positions(tekis);
        self.0.on_event(e, sim, Creatures { positions: &positions, table_sounds: &[] });
    }
}
