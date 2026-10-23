//! The S.S. Dolphin and its 30 parts.
//!
//! - The ship is `objects/ufo/ufo0705.mod` with its animations (`gameSetup.cpp` `shapeList2`).
//! - Parts are pellets whose config `p00` is 3 (`parms/pelMgr.bin`). Their models come from
//!   `objects/pelAnim.bin` (`PelletAnimInfo::read`: id, creation type, teki type, override
//!   joint, then a `Parameters` block with `x00` folder and `x01` file name;
//!   `createShapeObject` loads `objects/<folder>/<file>.mod` and the AnimMgr `<file>.bin`).
//! - Their order is `_ufoIDTable` (pelletMgr.cpp): 5 exterior (`ust*`, change the ship's
//!   shape), 11 attached (`uf*`), 14 internal (`un*`, not shown on the ship).
//! - Text: `ufo_<x>_0.blo` when a part is first found, `auf_<x>_0.blo` describing it when
//!   collected (then `tu_tx24.blo`), by index (`bloFile_PartsGetOnly_*`,
//!   `bloFile_PartsInfo_*`, ogTutorialData.cpp). The Main Engine's discovery is `tu_tx14.blo`.

use crate::Error;
use crate::anim::{AnimInfo, AnimMgr};
use crate::pani::{KeyEvent, PaniPlayer};
use crate::particle::effects;
use crate::reader::Reader;

/// `_ufoIDTable`: part ids in index order.
pub const PART_IDS: [&str; 30] = [
    "ust1", "ust2", "ust3", "ust4", "ust5", "uf01", "uf02", "uf03", "uf04", "uf05", "uf06", "uf07",
    "uf08", "uf09", "uf10", "uf11", "un01", "un02", "un03", "un04", "un05", "un06", "un07", "un08",
    "un09", "un10", "un11", "un12", "un13", "un14",
];

/// The Main Engine (`UFOID_MainEngine`), the first part in the story.
pub const MAIN_ENGINE: usize = 4;

pub const SHIP_MODEL: &str = "objects/ufo/ufo0705.mod";
pub const SHIP_BUNDLE: &str = "objects/ufo/ufo0705.anm";
pub const SHIP_ANIM_MGR: &str = "objects/ufo/ufo0705.bin";
pub const PEL_ANIM: &str = "objects/pelAnim.bin";

/// Screen letters per part index: a..z, then a1..d1.
fn letter(i: usize) -> String {
    let l = (b'a' + (i % 26) as u8) as char;
    if i < 26 { l.to_string() } else { format!("{l}1") }
}

/// The screen shown when part `index` is first found (`bloFile_PartsGetOnly_*`).
pub fn found_screen(index: usize) -> String {
    match index {
        // The Main Engine's discovery is a tutorial page; the letters skip it.
        MAIN_ENGINE => "screen/eng_blo/tu_tx14.blo".into(),
        i if i < MAIN_ENGINE => format!("screen/eng_blo/ufo_{}_0.blo", letter(i)),
        i => format!("screen/eng_blo/ufo_{}_0.blo", letter(i - 1)),
    }
}

/// The screen describing part `index` once it's on the ship (`bloFile_PartsInfo_*`).
pub fn info_screen(index: usize) -> String {
    format!("screen/eng_blo/auf_{}_0.blo", letter(index))
}

/// Shown after the description when a part is collected (`bloFile_PartsGet_*`).
pub const COLLECTED_SCREEN: &str = "screen/eng_blo/tu_tx24.blo";
/// The message window frame (`ogScrTutorialMgr`: `tu_base.blo`).
pub const MESSAGE_BASE_SCREEN: &str = "screen/eng_blo/tu_base.blo";
/// Where screen textures live for the English screens.
pub const SCREEN_TEXTURES: &str = "screen/eng_tex";

/// One `PelletAnimInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct PelletAnimInfo {
    pub id: String,
    /// `PCT_*`: 0 resident, 1 load if used, 2 load with a teki.
    pub creation: i32,
    pub teki_type: i32,
    pub override_joint: i32,
    pub folder: String,
    pub file: String,
}

impl PelletAnimInfo {
    pub fn model(&self) -> String {
        format!("objects/{}/{}.mod", self.folder, self.file)
    }

    pub fn anim_mgr(&self) -> String {
        format!("objects/{}/{}.bin", self.folder, self.file)
    }

    pub fn bundle(&self) -> String {
        format!("objects/{}/{}.anm", self.folder, self.file)
    }
}

/// `PelletMgr::readAnimInfos` on `objects/pelAnim.bin`.
pub fn parse_pel_anim(data: &[u8]) -> Result<Vec<PelletAnimInfo>, Error> {
    let mut r = Reader::new(data);
    let n = r.i32()?.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        // `ID32::read` stores the bytes reversed.
        let mut id = r.take(4)?.to_vec();
        id.reverse();
        let creation = r.i32()?;
        let teki_type = r.i32()?;
        let override_joint = r.i32()?;
        let (mut folder, mut file) = ("pellets".to_string(), "noname".to_string());
        loop {
            let h = r.i32()?;
            if h == -1 {
                break;
            }
            let b = h.to_be_bytes();
            if &b[..3] == b"x00" || &b[..3] == b"x01" {
                let len = r.i32()?.max(0) as usize;
                let s = r.take(len)?;
                let s = String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]).into_owned();
                if b[2] == b'0' {
                    folder = s;
                } else {
                    file = s;
                }
            } else {
                r.skip(b[3] as usize)?;
            }
        }
        out.push(PelletAnimInfo {
            id: String::from_utf8_lossy(&id).into_owned(),
            creation,
            teki_type,
            override_joint,
            folder,
            file,
        });
    }
    Ok(out)
}

/// The part's joint on the ship (`ufo0705.h`: `null_<file>`), for exterior and attached parts.
pub fn ship_joint(file: &str) -> String {
    format!("null_{file}")
}

/// `UfoShapeObject`: the joints whose subtrees each of the ship's 8 animation contexts drives,
/// in the order they're overridden (later ones win; 37 sits under 36).
pub const CONTEXT_JOINTS: [usize; 8] = [0, 38, 36, 19, 32, 37, 29, 53];
/// `UfoItem::UfoItem`: the joints whose materials the 4 level lights animate
/// (`instanceMaterials`).
pub const LIGHT_JOINTS: [usize; 4] = [15, 52, 51, 40];

/// `UfoMotion` (PaniAnimator.h): AnimMgr indices in `ufo0705.bin`.
pub mod motion {
    pub const WAIT_TUTORIAL: usize = 0;
    pub const WAIT: usize = 1;
    pub const HENKA1: usize = 3;
    pub const HENKA4B: usize = 10;
    /// `UfoItem::startTakeoff`: pm_st_l1-3 by ship level 1-5 (level 0 uses the first).
    pub const TAKEOFF: [usize; 5] = [11, 11, 12, 13, 13];
    pub const OPEN_CLOSE: usize = 16;
}

/// `PelletMotion` (PaniAnimator.h): AnimMgr indices in a part's `<file>.bin`.
pub mod part_motion {
    pub const CARRY: usize = 0;
    pub const APPEAR: usize = 1;
    pub const AFTER: usize = 2;
    pub const PISTON: usize = 3;
    pub const SPECIAL: usize = 4;
}

/// Something the ship's animations ask the host to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShipCue {
    /// `playEventSound(ufo, ...)`
    Sound(&'static str),
    /// Stop a sound the ship is playing.
    StopSound(&'static str),
    /// `effectMgr->create` at the ship's position raised by `lift`, with
    /// `setOrientedNormalVector(normal)` when given.
    Effect {
        fx: crate::particle::EffectDef,
        lift: f32,
        normal: Option<glam::Vec3>,
    },
    /// An effect where part `part` sits on the ship (`UfoParts::mRepairEffectPosition`).
    PartEffect { fx: crate::particle::EffectDef, part: usize },
    /// An effect that follows the ship model's collision sphere `sphere` (`setEmitPosPtr` on
    /// its centre). It runs until the host stops it; a new one in the same `slot` replaces it
    /// (`setJetEffect` kills the old flames first).
    Attached {
        fx: crate::particle::EffectDef,
        sphere: &'static str,
        normal: Option<glam::Vec3>,
        slot: u8,
    },
    /// An effect at `offset` in joint `joint`'s space, emitting along `dir` in the ship's
    /// space (`setEmitDir(dir.rotate(mWorldMtx))`).
    JointEffect {
        fx: crate::particle::EffectDef,
        joint: usize,
        offset: glam::Vec3,
        dir: glam::Vec3,
        normal: Option<glam::Vec3>,
    },
    /// A command to the cutscene sound track (`Jac_DemoSound`) for demo `demo`.
    DemoSound { demo: u16, cmd: u16 },
    /// The cutscene sound track goes quiet (`__Jac_FinishDemo`).
    EndDemoSound,
    /// Every [`ShipCue::Attached`] effect stops (`setJetEffect(0)`).
    StopAttached,
}

/// One part's animation once it's on the ship (`PlayerState::UfoParts`): the lower animator
/// drives the whole model, the upper (`PelletMotionFlags::UsePiston`) the subtree under its
/// override joint.
#[derive(Debug, Clone)]
pub struct PartAnim {
    pub lower: PaniPlayer,
    pub upper: Option<PaniPlayer>,
    /// `mMotionSpeed`, the lower animator's; the upper always runs at 30.
    speed: f32,
}

/// `UfoItem::LightAnimator`: a frame for the level lights' material animations.
#[derive(Debug, Clone, Copy, Default)]
struct Light {
    frame: f32,
    speed: f32,
    level: u8,
}

/// The S.S. Dolphin's animation state: `UfoAnimator`'s 8 contexts, the 4 level lights, and
/// the parts on board. It only animates; the sim decides when parts arrive.
#[derive(Debug, Clone)]
pub struct ShipAnimator {
    ship: Vec<AnimInfo>,
    /// Per part index: its AnimMgr entries and whether it has an upper animator.
    part_infos: Vec<Option<(Vec<AnimInfo>, bool)>>,
    ctx: [PaniPlayer; 8],
    speed: [f32; 8],
    /// Context 0 reports keys only when the ship itself started it (`PaniMotionInfo(id, this)`).
    listening: bool,
    lights: [Light; 4],
    pub level: u8,
    parts: Vec<Option<PartAnim>>,
    /// `PlayerState::mCurrentRepairingPart`
    repairing: Option<usize>,
    /// `isUfoBroken`: no parts on board.
    broken: bool,
    /// `hasUfoLeftControl` / `hasUfoRightControl`: Ionium Jet #1 / #2 are on board.
    ionium: [bool; 2],
    /// Frames since the take off started.
    takeoff_time: Option<f32>,
    /// Landing: frames left of the take off motion, played backwards.
    landing: Option<f32>,
}

const SPEED: f32 = 30.0;

/// `UfoParts::animationKeyUpdated` picks sounds by these ids.
const MAIN_ENGINE_ID: &str = "ust5";
const RADAR_ID: &str = "uf01";
const SATELLITE_ID: &str = "uf03";
/// `PlayerState::startSpecialMotions`: the Bowsprit, Whimsical Radar, Interstellar Radio and
/// Guard Satellite.
const SPECIAL_IDS: [&str; 4] = ["ust1", "uf01", "uf02", "uf03"];
/// `UFOID_IoniumJet1` / `2` (`PlayerState::getUfoParts` sets the left / right control flags).
const IONIUM_JET_IDS: [&str; 2] = ["uf10", "uf11"];
/// The engines' collision spheres (`setJetEffect`).
const ENGINES: [&str; 4] = ["eng1", "eng2", "eng3", "eng4"];
/// The side thrusters: joints 49 (left) and 48 (right), 14 up (`UfoItem::demoDraw`).
const PCA_JOINTS: [usize; 2] = [49, 48];
/// The take off's sound is the end-of-day scene's (`DEMOID_ChalDayEndPractice`,
/// cinemas/demo32.cin, which starts with `DemoEventUFO::Takeoff` at frame 0): its timed
/// commands (pikidemo.c `demo32`).
const TAKEOFF_DEMO: u16 = 32;
const TAKEOFF_DEMO_SOUNDS: [(f32, u16); 2] = [(0.0, 0), (160.0, 1)];

impl ShipAnimator {
    /// Repair queue completion, including the fitting motion's finish key.
    pub fn repair_finished(&self) -> bool {
        self.repairing.is_none() && self.ctx[0].is_finished()
    }
    /// `ship` is `ufo0705.bin`; `parts[i]` is part `i`'s AnimMgr and whether it has an
    /// override joint (`PelletAnimInfo::mOverrideJoint != -1`).
    pub fn new(ship: &AnimMgr, parts: Vec<Option<(AnimMgr, bool)>>) -> Self {
        let ship = ship.infos.clone();
        let idle = ship.first().map(|i| PaniPlayer::start(i, motion::WAIT_TUTORIAL));
        let idle = idle.unwrap_or_else(|| PaniPlayer::start(&AnimInfo::default(), 0));
        let n = parts.len();
        ShipAnimator {
            ctx: std::array::from_fn(|_| idle.clone()),
            ship,
            part_infos: parts.into_iter().map(|p| p.map(|(m, upper)| (m.infos, upper))).collect(),
            speed: [0.0; 8],
            listening: false,
            lights: [Light::default(); 4],
            level: 0,
            parts: vec![None; n],
            repairing: None,
            broken: true,
            ionium: [false; 2],
            takeoff_time: None,
            landing: None,
        }
    }

    fn start_ctx(&mut self, i: usize, m: usize) {
        if let Some(info) = self.ship.get(m) {
            self.ctx[i] = PaniPlayer::start(info, m);
        }
    }

    fn last_frame(&mut self, i: usize) {
        if let Some(info) = self.ship.get(self.ctx[i].motion) {
            self.ctx[i].set_last_frame(info);
        }
    }

    /// `UfoItem::startAI`: the ship as it stands when the level starts, with `on_board` parts
    /// already fitted and showing `level`.
    pub fn start(&mut self, on_board: &[u32], level: u8, cues: &mut Vec<ShipCue>) {
        self.set_up(on_board, level);
        self.stand(cues);
    }

    /// Context 0 standing still, with the ship's hum.
    fn stand(&mut self, cues: &mut Vec<ShipCue>) {
        self.start_ctx(0, if self.broken { motion::WAIT_TUTORIAL } else { motion::WAIT });
        self.listening = false;
        self.speed[0] = 0.0;
        cues.push(ShipCue::Sound(if self.broken { "SE_UFO_DESTROY" } else { "SE_UFO_IDLING" }));
    }

    /// `on_board` parts fitted, the lights and upgrade motions for `level`.
    fn set_up(&mut self, on_board: &[u32], level: u8) {
        self.takeoff_time = None;
        self.landing = None;
        self.repairing = None;
        self.parts.iter_mut().for_each(|p| *p = None);
        self.broken = on_board.is_empty();
        self.speed = [0.0; 8];
        for &p in on_board {
            self.fit(p as usize);
        }
        self.level = level;
        self.light_level(level);
        self.init_flag_motions(level);
    }

    /// Puts a part on board, waiting at the start of its Appear motion (`getUfoParts`).
    /// Returns whether it shows on the ship.
    fn fit(&mut self, part: usize) -> bool {
        // `naka` parts have no ship joint (`mRepairAnimJointIndex == -1`) and aren't drawn.
        if part >= 16 {
            return false;
        }
        let Some(Some((infos, upper))) = self.part_infos.get(part) else { return false };
        let at = |m: usize| infos.get(m).map(|i| PaniPlayer::start(i, m));
        let Some(lower) = at(part_motion::APPEAR) else { return false };
        let upper = if *upper { at(part_motion::PISTON) } else { None };
        self.parts[part] = Some(PartAnim { lower, upper, speed: 0.0 });
        true
    }

    /// `UfoParts::startMotion(lower, upper)` at full speed.
    fn part_motion(&mut self, part: usize, lower: usize, upper: usize) {
        let Some(Some((infos, has_upper))) = self.part_infos.get(part) else { return };
        let Some(p) = self.parts.get_mut(part).and_then(Option::as_mut) else { return };
        if let Some(i) = infos.get(lower) {
            p.lower = PaniPlayer::start(i, lower);
        }
        if *has_upper && let Some(i) = infos.get(upper) {
            p.upper = Some(PaniPlayer::start(i, upper));
        }
        p.speed = SPEED;
    }

    /// `PlayerState::startAfterMotions`
    fn after_motions(&mut self) {
        for i in 0..self.parts.len() {
            if self.parts[i].is_some() {
                self.part_motion(i, part_motion::AFTER, part_motion::AFTER);
            }
        }
    }

    /// `PlayerState::startSpecialMotions` (`startUfoPartsMotion(id, Special, false)`).
    fn special_motions(&mut self) {
        for id in SPECIAL_IDS {
            if let Some(i) = PART_IDS.iter().position(|p| *p == id) {
                self.part_motion(i, part_motion::SPECIAL, part_motion::SPECIAL);
            }
        }
    }

    /// `UfoItem::lightLevelFlag` / `LightAnimator::start`
    fn light_level(&mut self, level: u8) {
        for l in &mut self.lights {
            l.level = level;
            l.frame = if level > 0 { (level - 1) as f32 * 20.0 } else { 19.0 };
            l.speed = if level > 0 { SPEED } else { 0.0 };
        }
    }

    /// `UfoAnimator::initFlagMotions`: every level's upgrade motion, those already reached
    /// shown at their end.
    fn init_flag_motions(&mut self, level: u8) {
        for i in 1..8 {
            self.start_ctx(i, i + 2);
            self.speed[i] = 0.0;
        }
        self.start_flag_motions(level);
        let done: &[usize] = match level {
            0 => &[],
            1 => &[1, 2],
            2 => &[4, 5, 1, 2],
            3 => &[7, 3, 4, 5, 1, 2],
            _ => &[6, 7, 3, 4, 5, 1, 2],
        };
        for &i in done {
            self.last_frame(i);
        }
        self.after_motions();
        if level == 5 {
            self.special_motions();
        }
    }

    /// `UfoAnimator::startFlagMotions`: runs the upgrade motions up to `level`.
    fn start_flag_motions(&mut self, level: u8) {
        if level == 0 {
            for i in 1..8 {
                self.start_ctx(i, i + motion::HENKA1 - 1);
                self.speed[i] = 0.0;
            }
            return;
        }
        if level >= 5 {
            self.start_ctx(7, motion::HENKA4B);
            self.special_motions();
        }
        let run: &[usize] = match level {
            1 => &[1, 2],
            2 => &[4, 5, 1, 2],
            3 => &[7, 3, 4, 5, 1, 2],
            _ => &[6, 7, 3, 4, 5, 1, 2],
        };
        for &i in run {
            self.speed[i] = SPEED;
        }
    }

    /// `UfoItem::startLevelFlag`: the ship powers up to `level`.
    pub fn level_up(&mut self, level: u8, cues: &mut Vec<ShipCue>) {
        self.level = level;
        self.light_level(level);
        let normal = Some(glam::Vec3::Z);
        if level == 5 {
            for fx in effects::ROCKET_COMPLETE {
                cues.push(ShipCue::Effect { fx, lift: 0.0, normal });
            }
        } else {
            cues.push(ShipCue::Effect { fx: effects::ROCKET_C, lift: 60.0, normal });
        }
        self.start_flag_motions(level);
    }

    /// `UfoItem::startTakeoff`: the take off motion for the ship's level carries it up and
    /// away, and the parts with special motions start them (`startSpecialMotions`).
    /// `on_board` decides the side thrusters.
    pub fn take_off(&mut self, on_board: &[u32], cues: &mut Vec<ShipCue>) {
        let idx = (self.level as usize).saturating_sub(1).min(4);
        self.start_ctx(0, motion::TAKEOFF[idx]);
        self.speed[0] = SPEED;
        self.listening = true;
        self.repairing = None;
        self.special_motions();
        let has = |id: &str| PART_IDS.iter().position(|p| *p == id).is_some_and(|i| on_board.contains(&(i as u32)));
        self.ionium = IONIUM_JET_IDS.map(has);
        // The take off scene silences the field (`Jac_StartDemo` -> `Jac_InitAllEvent`),
        // which stops the ship's hum.
        cues.push(ShipCue::StopSound("SE_UFO_IDLING"));
        cues.push(ShipCue::StopSound("SE_UFO_DESTROY"));
        self.takeoff_time = Some(0.0);
    }

    /// Comes back down. Not in the game, where the ship is simply there the next day
    /// (`UfoItem::startAI`, with `EFF_Rocket_Land`): here it plays its take off motion
    /// backwards, flames on and roaring, and lands in that dust where the flames lit on the
    /// way up.
    pub fn land(&mut self, on_board: &[u32], level: u8, cues: &mut Vec<ShipCue>) {
        self.set_up(on_board, level);
        let idx = (level as usize).saturating_sub(1).min(4);
        self.start_ctx(0, motion::TAKEOFF[idx]);
        self.listening = false;
        self.speed[0] = 0.0;
        let end = self.ship.get(motion::TAKEOFF[idx]).and_then(|i| i.keys.last()).map_or(0.0, |&k| k as f32);
        self.ctx[0].frame = end;
        self.landing = Some(end);
        self.jets(cues);
        self.booster(cues);
        cues.push(ShipCue::DemoSound { demo: TAKEOFF_DEMO, cmd: TAKEOFF_DEMO_SOUNDS[0].1 });
    }

    /// The wreck's resting pose (context 0's motion and frame once it stands, `stand`'s `WAIT_TUTORIAL`), for a
    /// host that crash-lands the broken ship in it.
    pub fn wreck_context(&self) -> (usize, f32) {
        let frame = self.ship.get(motion::WAIT_TUTORIAL).and_then(|i| i.keys.first()).map_or(0.0, |&k| k as f32);
        (motion::WAIT_TUTORIAL, frame)
    }

    /// Whether the ship is landing.
    pub fn landing(&self) -> bool {
        self.landing.is_some()
    }

    /// Drive the reverse rocket motion from a host's normalized landing clock. Advancing
    /// by the difference retains touchdown/engine cues and the eight animation contexts.
    pub fn land_progress(&mut self, progress: f32, cues: &mut Vec<ShipCue>) {
        let end = self.ship.get(self.ctx[0].motion).and_then(|i| i.keys.last()).copied().unwrap_or(0) as f32;
        if let Some(left) = self.landing {
            let target = end * (1.0 - progress.clamp(0.0, 1.0));
            self.update((left - target).max(0.0) / SPEED, cues);
        }
    }

    /// The flames go out where they lit on the way up (the motion's key 0).
    fn touchdown_frame(&self) -> f32 {
        let info = self.ship.get(self.ctx[0].motion);
        let key = info.and_then(|i| i.event_keys.iter().find(|k| k.event == 1 && k.value == 0));
        key.map_or(0.0, |k| k.frame as f32)
    }

    /// Whether the ship is taking off.
    pub fn taking_off(&self) -> bool {
        self.takeoff_time.is_some()
    }

    /// `UfoItem::suckMe`: a part starts going in; the ship plays up to its Action0 key.
    pub fn suck_start(&mut self) {
        self.start_ctx(0, if self.broken { motion::WAIT_TUTORIAL } else { motion::WAIT });
        self.listening = true;
        self.speed[0] = SPEED;
    }

    /// `UfoItem::finishSuck`: part `part` is in, making the ship `level`. A visible part
    /// waits to fit itself until the ship's motion ends (`ufoAssignStart`); a new level powers
    /// the ship up (in the game the upgrade movie does, at its `Henka` event).
    pub fn suck_finish(&mut self, part: usize, level: u8, cues: &mut Vec<ShipCue>) {
        let shown = self.fit(part);
        self.repairing = shown.then_some(part);
        self.broken = false;
        self.speed[0] = SPEED;
        if level != self.level {
            self.level_up(level, cues);
        }
    }

    /// `PlayerState::ufoAssignStart`
    fn assign_start(&mut self) {
        if let Some(part) = self.repairing.take() {
            self.part_motion(part, part_motion::APPEAR, part_motion::PISTON);
        }
    }

    /// One frame of `dt` seconds: `UfoItem::update`'s `updateAnimation`, the lights
    /// (`LightAnimator::update`) and the parts (`renderParts`).
    pub fn update(&mut self, dt: f32, cues: &mut Vec<ShipCue>) {
        if let Some(left) = self.landing {
            let now = (left - SPEED * dt).max(0.0);
            let down = self.touchdown_frame();
            if left > down && now <= down {
                cues.push(ShipCue::StopAttached);
                if !self.broken {
                    cues.push(ShipCue::Effect { fx: effects::ROCKET_LAND, lift: 0.0, normal: None });
                }
            }
            self.ctx[0].frame = now;
            if now <= 0.0 {
                self.landing = None;
                cues.push(ShipCue::EndDemoSound);
                self.stand(cues);
            } else {
                self.landing = Some(now);
            }
        }
        if let Some(t) = self.takeoff_time.as_mut() {
            let step = SPEED * dt;
            for (at, cmd) in TAKEOFF_DEMO_SOUNDS {
                if at >= *t && at < *t + step {
                    cues.push(ShipCue::DemoSound { demo: TAKEOFF_DEMO, cmd });
                }
            }
            *t += step;
        }
        let mut ev = Vec::new();
        for i in 0..8 {
            let Some(info) = self.ship.get(self.ctx[i].motion) else { continue };
            self.ctx[i].animate(info, self.speed[i] * dt, &mut ev);
            if i == 0 && self.listening {
                for e in std::mem::take(&mut ev) {
                    self.ship_key(e, cues);
                }
            }
            ev.clear();
        }

        let mut blink = false;
        for l in &mut self.lights {
            if l.level == 0 {
                continue;
            }
            let base = 20.0 * (l.level - 1) as f32;
            l.frame += l.speed * dt;
            if l.frame >= 20.0 + base - 1.0 {
                blink = true;
                l.frame = base;
            }
        }
        if blink {
            // All four lights play it at once; once is enough.
            cues.push(ShipCue::Sound(if self.level < 5 { "SE_UFO_LIGHT" } else { "SE_UFO_LIGHT2" }));
        }

        for part in 0..self.parts.len() {
            let Some(Some((infos, _))) = self.part_infos.get(part) else { continue };
            let Some(p) = self.parts[part].as_mut() else { continue };
            if let Some(info) = infos.get(p.lower.motion) {
                p.lower.animate(info, p.speed * dt, &mut ev);
            }
            // The upper animator has no listener.
            if let Some(u) = p.upper.as_mut()
                && let Some(info) = infos.get(u.motion)
            {
                u.animate(info, SPEED * dt, &mut Vec::new());
            }
            let motion = p.lower.motion;
            for e in std::mem::take(&mut ev) {
                self.part_key(part, motion, e, cues);
            }
        }
    }

    /// `UfoItem::animationKeyUpdated` for the motions it reacts to.
    fn ship_key(&mut self, e: KeyEvent, cues: &mut Vec<ShipCue>) {
        let m = self.ctx[0].motion;
        if motion::TAKEOFF.contains(&m) {
            if let KeyEvent::Effect(v) = e {
                self.takeoff_key(v, cues);
            }
            return;
        }
        if m != motion::WAIT_TUTORIAL && m != motion::WAIT && m != motion::OPEN_CLOSE {
            return;
        }
        match e {
            KeyEvent::Action(0) => self.speed[0] = 0.0,
            KeyEvent::Finished => {
                if m != motion::OPEN_CLOSE {
                    self.assign_start();
                }
                self.speed[0] = 0.0;
                // `kiso_t` only shudders and ends as wrecked as it began; in the game the ship
                // first stands up on `kiso` after the upgrade movie, the next day (`startAI`).
                // Stand it up here instead.
                if m == motion::WAIT_TUTORIAL && !self.broken {
                    self.start_ctx(0, motion::WAIT);
                    self.listening = false;
                    cues.push(ShipCue::StopSound("SE_UFO_DESTROY"));
                    cues.push(ShipCue::Sound("SE_UFO_IDLING"));
                }
            }
            KeyEvent::Effect(0) if m == motion::WAIT_TUTORIAL => {
                for fx in effects::ROCKET_NKE {
                    cues.push(ShipCue::Effect { fx, lift: 0.0, normal: None });
                }
            }
            KeyEvent::Effect(2) if m != motion::OPEN_CLOSE => {
                cues.push(ShipCue::Effect { fx: effects::CLOUD_OF_DUST_1, lift: 0.0, normal: None });
            }
            _ => {}
        }
    }

    /// `UfoItem::animationKeyUpdated` for the take off motions' effect keys.
    fn takeoff_key(&mut self, v: u8, cues: &mut Vec<ShipCue>) {
        let ground = |fx| ShipCue::Effect { fx, lift: 0.0, normal: None };
        match v {
            0 => self.jets(cues),
            1 => {
                match self.level {
                    0 | 1 => cues.extend([ground(effects::ROCKET_JET_G[0]), ground(effects::ROCKET_FKM1)]),
                    2 | 3 => cues.extend([ground(effects::ROCKET_JET_G[1]), ground(effects::ROCKET_FKM1)]),
                    4 => cues.extend([ground(effects::ROCKET_JET_G[2]), ground(effects::ROCKET_FKM1)]),
                    _ => cues.extend(effects::ROCKET_BOOST_GROUND.map(ground)),
                }
                // The game falls through to key 9 here.
                self.booster(cues);
            }
            9 => self.booster(cues),
            2 if self.ionium[1] => self.side_thruster(1, -glam::Vec3::X, cues),
            3 if self.ionium[0] => self.side_thruster(0, glam::Vec3::X, cues),
            _ => {}
        }
    }

    /// `UfoItem::setJetEffect(level, false)`: flames on the four engines, picked by the ship's
    /// level (below level 2 each engine has its own).
    fn jets(&self, cues: &mut Vec<ShipCue>) {
        let level = self.level.min(5) as usize;
        for (i, sphere) in ENGINES.into_iter().enumerate() {
            let row = if level <= 1 { 4 + i } else { level - 2 };
            let [fb, fa] = effects::ROCKET_JETS[row];
            cues.push(ShipCue::Attached { fx: fb, sphere, normal: None, slot: i as u8 * 2 });
            cues.push(ShipCue::Attached { fx: fa, sphere, normal: Some(glam::Vec3::X), slot: i as u8 * 2 + 1 });
        }
    }

    /// The finished ship's booster at 'gcen'.
    fn booster(&self, cues: &mut Vec<ShipCue>) {
        if self.level != 5 {
            return;
        }
        for (i, fx) in effects::ROCKET_BOOSTER.into_iter().enumerate() {
            let normal = (i == 1).then_some(glam::Vec3::X);
            cues.push(ShipCue::Attached { fx, sphere: "gcen", normal, slot: 8 + i as u8 });
        }
    }

    /// `UfoItem::setPca1Effect` (left) / `setPca2Effect` (right).
    fn side_thruster(&self, side: usize, dir: glam::Vec3, cues: &mut Vec<ShipCue>) {
        for fx in effects::ROCKET_PCA {
            cues.push(ShipCue::JointEffect {
                fx,
                joint: PCA_JOINTS[side],
                offset: glam::Vec3::new(0.0, 14.0, 0.0),
                dir,
                normal: Some(glam::Vec3::Y),
            });
        }
    }

    /// `PlayerState::UfoParts::animationKeyUpdated`
    fn part_key(&mut self, part: usize, motion: usize, e: KeyEvent, cues: &mut Vec<ShipCue>) {
        let id = PART_IDS.get(part).copied().unwrap_or("");
        match e {
            KeyEvent::LoopEnd if motion == part_motion::PISTON => match id {
                RADAR_ID => cues.push(ShipCue::Sound("SE_UFO_ANTENNA")),
                SATELLITE_ID => cues.push(ShipCue::Sound("SE_UFO_SATELLITE")),
                _ => {}
            },
            KeyEvent::LoopEnd => {
                // The Main Engine falls through to the radar's sound as well, as in the game.
                if id == MAIN_ENGINE_ID {
                    cues.push(ShipCue::Sound("SE_UFO_ENGINE"));
                }
                if id == MAIN_ENGINE_ID || id == RADAR_ID {
                    cues.push(ShipCue::Sound("SE_UFO_RADER"));
                }
            }
            KeyEvent::Finished => self.part_motion(part, part_motion::AFTER, part_motion::AFTER),
            KeyEvent::Effect(v) if motion == part_motion::APPEAR && v < 2 => {
                cues.push(ShipCue::PartEffect { fx: effects::UFO_PART_ASSIGN[v as usize], part });
            }
            _ => {}
        }
    }

    /// Context `i`'s motion (an AnimMgr index) and frame.
    pub fn context(&self, i: usize) -> (usize, f32) {
        (self.ctx[i].motion, self.ctx[i].frame)
    }

    /// Light `i`'s material animation frame.
    pub fn light_frame(&self, i: usize) -> f32 {
        self.lights[i].frame
    }

    /// Part `part`'s animation if it's showing on the ship.
    pub fn part(&self, part: usize) -> Option<&PartAnim> {
        self.parts.get(part)?.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screens_follow_the_tables() {
        assert_eq!(found_screen(0), "screen/eng_blo/ufo_a_0.blo");
        assert_eq!(found_screen(3), "screen/eng_blo/ufo_d_0.blo");
        assert_eq!(found_screen(4), "screen/eng_blo/tu_tx14.blo");
        assert_eq!(found_screen(5), "screen/eng_blo/ufo_e_0.blo");
        assert_eq!(found_screen(26), "screen/eng_blo/ufo_z_0.blo");
        assert_eq!(found_screen(29), "screen/eng_blo/ufo_c1_0.blo");
        assert_eq!(info_screen(0), "screen/eng_blo/auf_a_0.blo");
        assert_eq!(info_screen(29), "screen/eng_blo/auf_d1_0.blo");
    }
}
