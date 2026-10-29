//! Candypop Buds in the Pikmin 1 view: Pikmin 2's `Pom` model (`enemy/data/Pom/model/enemy.bmd`, from
//! `libsms::extract::p2_enemy`), with the petals in the bud's colour, posed on the disc's own clips for the bud's phase
//! (`PomState.cpp`: `type1` opens, `type2` closes, `type3` spits, `type4` swings, `dead` withers; the closed bud is
//! `wait`). The rules are `libpikmin::bulb`.
//!
//! Spec: `Pom::Obj::setPomColor` and `changeMaterial` (Pom.cpp): the petal material `hanabira1_v` has TEV colour 0 set to
//! the bud's `mRgbColor` (Blue 50,50,255, Red 255,20,20, Yellow 255,255,20, Purple 28,0,52, White 200,255,220).
//!
//! Host adaptation: the game recolours the material at run time; here one body per colour is baked from a copy of the
//! model whose colour table entry for that material is rewritten (the copies are written beside the model in the
//! ignored cache, once).

use std::path::Path;

use glam::Mat4;
use libpikmin::bulb::{Bulb, BulbState};
use libpikmin::PikiColor;
use libpikmin_assets::model::Batch;
use libsms_assets::jpa2::Jpc;
use libsms_present::enemy::EnemyBody;

use crate::effects::Flat;
use crate::navilight::{Glow, glow};
use crate::tev;
use glam::Vec3;

const MODEL: &str = "enemy/data/Pom/model/enemy.bmd";
const CLIPS: &str = "enemy/data/Pom/anim";
/// The petal material's colour table entry, as the model has it (red, alpha 0), as big-endian shorts.
const PETAL: [u8; 8] = [0, 255, 0, 20, 0, 20, 0, 0];

/// `Pom::Obj::setPomColor`'s `mRgbColor` for each colour (Blue, Red, Yellow, Purple, White).
pub const PETAL_COLOURS: [[u8; 3]; 5] = [[50, 50, 255], [255, 20, 20], [255, 255, 20], [28, 0, 52], [200, 255, 220]];

/// One colour's body in the view's tables.
pub struct BulbBody {
    pub body: EnemyBody,
    pub model: u32,
    pub tex_base: i32,
}

/// The bud in each of Pikmin's five colours, by [`PikiColor::index`].
pub struct BulbSet {
    pub bodies: Vec<BulbBody>,
}

/// The model's bytes with the petals' colour table entry rewritten to `rgb`.
pub fn recoloured(model: &[u8], rgb: [u8; 3]) -> Result<Vec<u8>, String> {
    let at: Vec<usize> = model.windows(PETAL.len()).enumerate().filter(|(_, w)| *w == PETAL).map(|(i, _)| i).collect();
    let [at] = at[..] else { return Err(format!("the petal colour appears {} times in the model", at.len())) };
    let mut out = model.to_vec();
    for (k, c) in rgb.iter().enumerate() {
        out[at + 2 * k] = 0;
        out[at + 2 * k + 1] = *c;
    }
    Ok(out)
}

impl BulbSet {
    /// Whether the Pikmin 2 cache `dir` has the files.
    pub fn available(dir: &Path) -> bool {
        dir.join(MODEL).is_file() && dir.join(CLIPS).join("type1.bca").is_file()
    }

    /// Bakes the five bodies; `first_model` and `first_tex` are where they will sit in the view's tables, in colour order.
    pub fn load(dir: &Path, first_model: u32, first_tex: i32) -> Result<Self, String> {
        let original = std::fs::read(dir.join(MODEL)).map_err(|e| format!("{MODEL}: {e}"))?;
        let mut bodies = Vec::new();
        let mut tex = first_tex;
        for (i, rgb) in PETAL_COLOURS.iter().enumerate() {
            let rel = format!("enemy/data/Pom/model/enemy_c{i}.bmd");
            let path = dir.join(&rel);
            if !path.is_file() {
                std::fs::write(&path, recoloured(&original, *rgb)?).map_err(|e| format!("{rel}: {e}"))?;
            }
            let body = EnemyBody::load_swapped(dir, &rel, CLIPS, &[])?;
            let next = tex + body.textures.list.len() as i32;
            bodies.push(BulbBody { body, model: first_model + i as u32, tex_base: tex });
            tex = next;
        }
        Ok(BulbSet { bodies })
    }

    pub fn body(&self, colour: PikiColor) -> Option<&BulbBody> {
        self.bodies.get(colour.index())
    }

    pub fn batches(&self, index: usize) -> Vec<Batch> {
        self.bodies.get(index).map_or_else(Vec::new, |b| crate::pianta::batches_of(&b.body.model, &b.body.bind_world(), 1.0))
    }

    pub fn textures(&self, index: usize) -> Vec<tev::Baked> {
        self.bodies.get(index).map_or_else(Vec::new, |b| crate::pianta::textures_of(&b.body.textures))
    }

    /// The clip and frame a bud in `state` shows, 30 frames a second.
    pub fn clip(state: BulbState) -> (&'static str, f32) {
        match state {
            BulbState::Open { t } => ("type1", t * 30.0),
            // The swing plays when a Pikmin has just gone in, then the bud stands open as the open clip ends.
            BulbState::Swing { t } => if t * 30.0 < 20.0 { ("type4", t * 30.0) } else { ("type1", f32::MAX) },
            BulbState::Close { t } => ("type2", t * 30.0),
            BulbState::Shot { t, .. } => ("type3", t * 30.0),
            BulbState::Dead { t } => ("dead", t * 30.0),
        }
    }

    /// The joints of `bulb` now, model space.
    pub fn joints(&self, bulb: &Bulb) -> Vec<Mat4> {
        let Some(b) = self.body(bulb.colour) else { return Vec::new() };
        let (name, frame) = Self::clip(bulb.state);
        let frames = b.body.clip_frames(name).unwrap_or(1.0);
        b.body.pose(name, frame.min((frames - 1.0).max(0.0))).unwrap_or_else(|| b.body.bind_world())
    }
}

// ---- sounds and effects ----------------------------------------------------------------------------------------

/// The sounds a bud makes, Pikmin 2's `SoundID.h` ids. The Pom enemy has none of its own in the decomp (they come from its
/// animation's sequence), so these are the Onion and sprout sounds that mean the same, all verified audible in
/// `libsms-audio/tests/pikmin2_bud_sounds.rs`: the petals open, a Pikmin goes in, the petals close, the seeds are spat out,
/// the bud withers.
pub const OPEN: u32 = 0x281B;
pub const ENTER: u32 = 0x281D;
pub const CLOSE: u32 = 0x281A;
pub const EJECT: u32 = 0x2816;
pub const WITHER: u32 = 0x281C;

/// What a bud did this frame that looks like something: the stand-in effect to draw (`efx::TEnemyDownSmoke` at the swing in
/// `Obj::createSwingSmokeEffect`, `efx::TOnyonLay` in `createShotEffect`, `efx::TPonDead` in `createPomDeadEffect`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Burst {
    /// A Pikmin went in: a puff.
    Swallow,
    /// The petals shut.
    Close,
    /// The seeds fly.
    Spit,
    /// It withers.
    Wither,
}

/// How long a burst lasts, seconds.
pub const BURST_SECONDS: f32 = 1.2;

/// Buds further than this from every captain, across the ground in sim units (3000 is 120 m at the RoR2 host's 0.04), are not
/// drawn, heard or lit.
pub const DRAW_DISTANCE: f32 = 3000.0;

/// What the view remembers of one bud between frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    tag: u8,
    fired: bool,
    used: u32,
}

fn tag_of(state: BulbState) -> (u8, bool) {
    match state {
        BulbState::Open { .. } => (0, false),
        BulbState::Swing { .. } => (1, false),
        BulbState::Close { .. } => (2, false),
        BulbState::Shot { fired, .. } => (3, fired),
        BulbState::Dead { .. } => (4, false),
    }
}

/// The moments of one bud in a frame: `prev` is what the view remembered (None the first time it is seen, when nothing is
/// announced: a stage's worth of buds appears at once, and a client meets the ones already there). Returns the sounds, the
/// burst, and what to remember. Pure, so it is tested without a window.
pub fn moments(prev: Option<Track>, bulb: &Bulb) -> (Vec<u32>, Option<Burst>, Track) {
    let (tag, fired) = tag_of(bulb.state);
    let now = Track { tag, fired, used: bulb.used + bulb.swallowed };
    let Some(before) = prev else { return (Vec::new(), None, now) };
    let mut cues = Vec::new();
    let mut burst = None;
    if now.used > before.used && tag != 3 {
        cues.push(ENTER);
        burst = Some(Burst::Swallow);
    }
    if tag != before.tag {
        match tag {
            0 if before.tag == 3 => cues.push(OPEN),
            2 => {
                cues.push(CLOSE);
                burst = burst.or(Some(Burst::Close));
            }
            4 => {
                cues.push(WITHER);
                burst = Some(Burst::Wither);
            }
            _ => {}
        }
    }
    if tag == 3 && fired && !(before.tag == 3 && before.fired) {
        cues.push(EJECT);
        burst = Some(Burst::Spit);
    }
    (cues, burst, now)
}

/// The view's memory of the buds, by id.
pub type Tracks = std::collections::HashMap<u32, Track>;

/// `PID_OnyonPuffKira` (sparkles), `PID_OnyonLay_1` (the seeds let go), `PID_EnemyDownSmoke`, `PID_PonDead`.
pub const PUFF: u16 = 0x103;
pub const LAY: u16 = 0x101;
pub const SMOKE: u16 = 0x053;
pub const DEAD: u16 = 0x271;

/// A bud's light in its colour: brighter than the petals for the dark Purple, which would not show added to the screen.
pub const GLOW_TINT: [[f32; 3]; 5] = [[0.35, 0.45, 1.0], [1.0, 0.3, 0.25], [1.0, 0.95, 0.3], [0.65, 0.3, 1.0], [0.85, 1.0, 0.92]];

/// The stand-in effects: like Louie's navi light and the treasures' sparkles, one camera-facing additive glow from each
/// resource's own texture and colours, tinted with the bud's colour (the real ones are JPA2 emitters, which are not run).
pub struct BulbFx {
    puff: Glow,
    lay: Glow,
    smoke: Glow,
    dead: Glow,
}

impl BulbFx {
    /// Reads the four resources from the Pikmin 2 cache `dir` (`effects/game.jpc`).
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = [dir.join("effects/game.jpc"), dir.join("treasure/effects/ebisawa_game.jpc")]
            .into_iter().find(|p| p.is_file()).ok_or("no effects/game.jpc")?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let jpc = Jpc::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let need = |id: u16| glow(&jpc, id, 1.0).ok_or_else(|| format!("no resource {id:#x}"));
        Ok(BulbFx { puff: need(PUFF)?, lay: need(LAY)?, smoke: need(SMOKE)?, dead: need(DEAD)? })
    }

    /// Two twinkles circling the mouth of an open bud that has room, in its colour.
    pub fn twinkle(&mut self, flat: &mut Flat, feet: Vec3, colour: PikiColor, key: u32, time: f32, right: Vec3, up: Vec3) {
        let tint = GLOW_TINT[colour.index().min(4)];
        let phase = key as f32 * 1.3;
        for k in 0..2 {
            let a = time * 0.9 + phase + k as f32 * std::f32::consts::PI;
            let twinkle = 0.5 + 0.5 * (time * 3.1 + phase * 2.0 + k as f32 * 1.9).sin();
            let centre = feet + Vec3::new(a.cos() * RADIUS_FX, HEIGHT_FX * (0.75 + 0.2 * (a * 1.7).sin()), a.sin() * RADIUS_FX);
            self.puff.push_tinted(flat, centre, 2.5 + 3.5 * twinkle, 0.25 + 0.5 * twinkle, tint, right, up);
        }
    }

    /// A burst `age` seconds old for a bud of `colour` standing at `feet`.
    pub fn burst(&mut self, flat: &mut Flat, kind: Burst, feet: Vec3, colour: PikiColor, age: f32, right: Vec3, up: Vec3) {
        let tint = GLOW_TINT[colour.index().min(4)];
        let p = (age / BURST_SECONDS).clamp(0.0, 1.0);
        let mouth = feet + Vec3::Y * HEIGHT_FX;
        match kind {
            Burst::Swallow => self.puff.push_tinted(flat, mouth, 10.0 + 14.0 * p, 0.9 * (1.0 - p), tint, right, up),
            Burst::Close => self.smoke.push_tinted(flat, mouth, 10.0 + 8.0 * p, 0.5 * (1.0 - p), tint, right, up),
            Burst::Spit => {
                self.lay.push_tinted(flat, mouth + Vec3::Y * 10.0 * p, 14.0 + 30.0 * p, 1.0 * (1.0 - p), tint, right, up);
                self.puff.push_tinted(flat, mouth + Vec3::Y * 30.0 * p, 8.0 + 10.0 * p, 0.8 * (1.0 - p), tint, right, up);
            }
            Burst::Wither => self.dead.push_tinted(flat, feet + Vec3::Y * HEIGHT_FX * 0.5, 14.0 + 18.0 * p, 0.8 * (1.0 - p), tint, right, up),
        }
    }
}

/// Where the mouth is and how wide the twinkles circle, in sim units (the bud's `libpikmin::bulb::HEIGHT` and `RADIUS`).
const HEIGHT_FX: f32 = libpikmin::bulb::HEIGHT * 0.6;
const RADIUS_FX: f32 = libpikmin::bulb::RADIUS * 0.8;

#[cfg(test)]
mod tests {
    use super::*;
    use libpikmin_assets::Assets;

    fn dir() -> Option<std::path::PathBuf> {
        let dir = Assets::default_dir().join("p2");
        BulbSet::available(&dir).then_some(dir)
    }

    #[test]
    fn the_petal_colour_is_rewritten_in_one_place() {
        let mut model = vec![9u8; 40];
        model[10..18].copy_from_slice(&PETAL);
        let blue = recoloured(&model, [50, 50, 255]).unwrap();
        assert_eq!(&blue[10..18], &[0, 50, 0, 50, 0, 255, 0, 0]);
        assert_eq!(&blue[..10], &model[..10]);
        assert_eq!(&blue[18..], &model[18..]);
        assert!(recoloured(&[0; 16], [1, 2, 3]).is_err(), "no petal colour to rewrite");
    }

    #[test]
    fn the_timings_in_the_core_are_the_discs_clips() {
        let Some(dir) = dir() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let body = EnemyBody::load_swapped(&dir, MODEL, CLIPS, &[]).unwrap();
        let secs = |c: &str| body.clip_frames(c).unwrap() / 30.0;
        assert!((secs("type2") - libpikmin::bulb::CLOSE_TIME).abs() < 1e-3, "close");
        assert!((secs("type3") - libpikmin::bulb::SHOT_TIME).abs() < 1e-3, "shot");
        assert!((secs("dead") - libpikmin::bulb::WITHER_TIME).abs() < 1e-3, "wither");
        assert!(body.height() > 10.0, "a flower: {}", body.height());
    }

    #[test]
    fn each_colour_bakes_and_the_petals_differ() {
        let Some(dir) = dir() else { eprintln!("SKIP: Pikmin 2 enemy files are not extracted"); return };
        let set = BulbSet::load(&dir, 100, 50).unwrap();
        assert_eq!(set.bodies.len(), 5);
        let sums: Vec<u64> = (0..5).map(|i| set.textures(i).iter().map(|t| t.rgba.iter().map(|&b| b as u64).sum::<u64>()).sum()).collect();
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(sums[i], sums[j], "colours {i} and {j} bake the same");
            }
            assert!(!set.batches(i).is_empty());
        }
        let bulb = bud(BulbState::Shot { t: 0.5, fired: false }, 0, 0);
        assert_eq!(set.joints(&bulb).len(), 11);
    }

    fn bud(state: BulbState, used: u32, swallowed: u32) -> Bulb {
        Bulb {
            id: libpikmin::BulbId(1), pos: Vec3::ZERO, colour: PikiColor::Purple, queen: false, slots: 5, used, swallowed, same: 0,
            change: 0.0, state, owner: None,
        }
    }

    #[test]
    fn a_buds_moments_in_order() {
        // Met for the first time, nothing is announced, whatever it is doing.
        let (cues, burst, t) = moments(None, &bud(BulbState::Open { t: 3.0 }, 0, 0));
        assert!(cues.is_empty() && burst.is_none());
        // A Pikmin goes in.
        let (cues, burst, t) = moments(Some(t), &bud(BulbState::Swing { t: 0.0 }, 1, 1));
        assert_eq!((cues, burst), (vec![ENTER], Some(Burst::Swallow)));
        // Still swinging: quiet. A second one goes in.
        let (cues, _, t) = moments(Some(t), &bud(BulbState::Swing { t: 1.0 }, 1, 1));
        assert!(cues.is_empty());
        let (cues, _, t) = moments(Some(t), &bud(BulbState::Swing { t: 0.0 }, 2, 2));
        assert_eq!(cues, [ENTER]);
        // It shuts, spits (once), and opens again.
        let (cues, burst, t) = moments(Some(t), &bud(BulbState::Close { t: 0.0 }, 2, 2));
        assert_eq!((cues, burst), (vec![CLOSE], Some(Burst::Close)));
        let (cues, _, t) = moments(Some(t), &bud(BulbState::Shot { t: 0.1, fired: false }, 2, 2));
        assert!(cues.is_empty(), "the seeds have not flown yet");
        let (cues, burst, t) = moments(Some(t), &bud(BulbState::Shot { t: 0.7, fired: true }, 2, 0));
        assert_eq!((cues, burst), (vec![EJECT], Some(Burst::Spit)), "the slots coming back are not a Pikmin going in");
        let (cues, _, t) = moments(Some(t), &bud(BulbState::Shot { t: 1.0, fired: true }, 2, 0));
        assert!(cues.is_empty());
        let (cues, _, t) = moments(Some(t), &bud(BulbState::Open { t: 0.0 }, 2, 0));
        assert_eq!(cues, [OPEN]);
        // Full, it withers.
        let (cues, burst, _) = moments(Some(t), &bud(BulbState::Dead { t: 0.0 }, 2, 0));
        assert_eq!((cues, burst), (vec![WITHER], Some(Burst::Wither)));
    }

    #[test]
    fn the_effects_draw_tinted_additive_layers() {
        let Some(dir) = dir() else { eprintln!("SKIP: Pikmin 2 files are not extracted"); return };
        let mut fx = BulbFx::load(&dir).expect("the four resources");
        let mut flat = Flat::default();
        for kind in [Burst::Swallow, Burst::Close, Burst::Spit, Burst::Wither] {
            let before = flat.quads.len();
            fx.burst(&mut flat, kind, Vec3::ZERO, PikiColor::Blue, 0.3, Vec3::X, Vec3::Y);
            assert!(flat.quads.len() >= before + 2, "{kind:?} draws");
        }
        fx.twinkle(&mut flat, Vec3::ZERO, PikiColor::Red, 7, 1.0, Vec3::X, Vec3::Y);
        assert!(flat.quads.iter().all(|q| q.additive && q.color.iter().all(|c| c.is_finite() && *c >= 0.0)));
        // A blue bud's light is bluer than red.
        let blue = {
            let mut f = Flat::default();
            fx.burst(&mut f, Burst::Swallow, Vec3::ZERO, PikiColor::Blue, 0.0, Vec3::X, Vec3::Y);
            f.quads.iter().map(|q| q.color[2] - q.color[0]).sum::<f32>()
        };
        let red = {
            let mut f = Flat::default();
            fx.burst(&mut f, Burst::Swallow, Vec3::ZERO, PikiColor::Red, 0.0, Vec3::X, Vec3::Y);
            f.quads.iter().map(|q| q.color[2] - q.color[0]).sum::<f32>()
        };
        assert!(blue > red, "{blue} vs {red}");
        // At its end a burst has faded away.
        let mut f = Flat::default();
        fx.burst(&mut f, Burst::Spit, Vec3::ZERO, PikiColor::Blue, BURST_SECONDS, Vec3::X, Vec3::Y);
        assert!(f.quads.iter().all(|q| q.color[..3].iter().all(|c| c.abs() < 1e-6)));
    }
}
