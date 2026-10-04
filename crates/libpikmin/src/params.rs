//! Tuning parameters.
//!
//! `Default` gives the decomp's *code* defaults (the `Parm<T>` initialisers in NaviMgr.h,
//! PikiMgr.h, CPlate.h, AIConstant.h, CreatureProp.h). The shipped game overrides most of
//! them from `dataDir/parms/*.bin` on the disc. Use the `apply_*_bin` methods with bytes from
//! the user's own disc to get the real values (see `docs/behavior-spec.md` for the differences).
//!
//! File format, from `Parameters::read` (plugPikiColin/parameters.cpp): a sequence of
//! big-endian entries `header: i32 = (3-char id << 8) | byte size`, then the value, ending with
//! a header of -1. A file value goes to the *first* parameter with that id, in declaration
//! order, and unknown ids are skipped by size.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParmError {
    Truncated,
    BadSize { id: String, size: usize },
}

impl fmt::Display for ParmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParmError::Truncated => write!(f, "parm file ended early"),
            ParmError::BadSize { id, size } => write!(f, "parm {id} has unexpected size {size}"),
        }
    }
}

impl std::error::Error for ParmError {}

trait ParmValue: Sized {
    fn from_be(raw: &[u8]) -> Option<Self>;
}

impl ParmValue for f32 {
    fn from_be(raw: &[u8]) -> Option<Self> {
        Some(f32::from_be_bytes(raw.try_into().ok()?))
    }
}

impl ParmValue for i32 {
    fn from_be(raw: &[u8]) -> Option<Self> {
        Some(i32::from_be_bytes(raw.try_into().ok()?))
    }
}

/// Reads one `Parameters` block starting at `*pos`, calling `set` for each entry.
fn read_block(
    data: &[u8],
    pos: &mut usize,
    mut set: impl FnMut(&[u8; 3], &[u8]) -> Result<(), ParmError>,
) -> Result<(), ParmError> {
    loop {
        let header = data.get(*pos..*pos + 4).ok_or(ParmError::Truncated)?;
        *pos += 4;
        let header = i32::from_be_bytes(header.try_into().unwrap());
        if header == -1 {
            return Ok(());
        }
        let bytes = header.to_be_bytes();
        let id = [bytes[0], bytes[1], bytes[2]];
        let size = bytes[3] as usize;
        let raw = data.get(*pos..*pos + size).ok_or(ParmError::Truncated)?;
        *pos += size;
        set(&id, raw)?;
    }
}

macro_rules! parms {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty = $default:expr, $id:literal; )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $( $field: $default, )* }
            }
        }

        impl $name {
            /// Applies one file entry. Unknown ids are ignored, like the game does.
            fn set(&mut self, id: &[u8; 3], raw: &[u8]) -> Result<(), ParmError> {
                $(
                    if id == $id {
                        self.$field = <$ty as ParmValue>::from_be(raw).ok_or_else(|| ParmError::BadSize {
                            id: String::from_utf8_lossy(id).into_owned(),
                            size: raw.len(),
                        })?;
                        return Ok(());
                    }
                )*
                Ok(())
            }

            fn read_block(&mut self, data: &[u8], pos: &mut usize) -> Result<(), ParmError> {
                read_block(data, pos, |id, raw| self.set(id, raw))
            }
        }
    };
}

parms! {
    /// `CreatureProp::Parms` (CreatureProp.h).
    pub struct CreatureParams {
        friction: f32 = 0.5, b"s00";
        wall_reflection: f32 = 0.5, b"s01";
        face_dir_adjust: f32 = 0.25, b"s02";
        /// Time constant for velocity easing: `vel += (target - vel) * dt / acceleration`
        /// (`Creature::moveVelocity`, creature.cpp).
        acceleration: f32 = 0.1, b"s03";
        bounce_factor: f32 = 0.3, b"s04";
    }
}

parms! {
    /// `NaviProp::Parms` (NaviMgr.h). Only the fields libpikmin uses.
    pub struct NaviParams {
        action_radius: f32 = 120.0, b"p00";
        whistle_max_radius: f32 = 100.0, b"p01";
        whistle_min_radius: f32 = 5.0, b"p53";
        whistle_expand_time: f32 = 0.2, b"p02";
        whistle_hold_time: f32 = 0.15, b"p03";
        move_speed: f32 = 170.0, b"p04";
        run_speed: f32 = 210.0, b"p56";
        throw_hold_max_time: f32 = 2.5, b"p09";
        throw_max_distance: f32 = 600.0, b"p10";
        throw_min_distance: f32 = 150.0, b"p11";
        throw_max_height: f32 = 100.0, b"p24";
        throw_min_height: f32 = 80.0, b"p25";
        yellow_throw_height: f32 = 200.0, b"p54";
        throw_flight_time: f32 = 1.0, b"p26";
        pluck_grab_range: f32 = 80.0, b"p37";
        piki_wait_range: f32 = 30.0, b"p39";
        piki_formation_change_range: f32 = 60.0, b"p40";
        body_collision_radius: f32 = 15.0, b"p21";
        ground_collision_radius: f32 = 16.0, b"p41";
        /// Declared before `clamp_stick_threshold`, so it receives the file's p48 value.
        cursor_move_delay_time: f32 = 2.0, b"p48";
        piki_impatience_time: f32 = 8.0, b"p49";
        neutral_stick_threshold: f32 = 0.1, b"p43";
        cursor_move_stick_threshold: f32 = 0.75, b"p44";
        /// Shares id p48 with `cursor_move_delay_time`, so files can never set it.
        clamp_stick_threshold: f32 = 0.85, b"p48";
        cursor_min_radius: f32 = 50.0, b"p45";
        cursor_max_radius: f32 = 300.0, b"p46";
        cursor_move_speed: f32 = 200.0, b"p47";
        health: f32 = 100.0, b"p50";
    }
}

parms! {
    /// `PikiProp::Parms` (PikiMgr.h). Only the fields libpikmin uses.
    pub struct PikiParams {
        min_move_speed: f32 = 84.0, b"p00";
        max_leaf_move_speed: f32 = 162.5, b"p01";
        max_flower_move_speed: f32 = 220.0, b"p54";
        max_bud_move_speed: f32 = 160.0, b"p65";
        blue_attack_power: f32 = 10.0, b"p04";
        red_attack_power: f32 = 15.0, b"p12";
        yellow_attack_power: f32 = 10.0, b"p13";
        min_piki_size: f32 = 6.0, b"p27";
        max_piki_size: f32 = 10.0, b"p34";
        inv_mass: f32 = 100.0, b"p28";
        collision_radius: f32 = 8.0, b"p39";
        formation_slip_range: f32 = 600.0, b"p29";
        formation_break_range: f32 = 1000.0, b"p30";
        max_lost_child_time: f32 = 10.0, b"p31";
        idle_attack_search_range: f32 = 100.0, b"p41";
        idle_work_search_range: f32 = 100.0, b"p42";
        max_flick_knockdown_time: f32 = 0.7, b"p43";
        min_flick_knockdown_time: f32 = 0.7, b"p74";
        flower_gravity_scale: f32 = 0.01, b"p48";
        post_work_join_party_range: f32 = 250.0, b"p61";
    }
}

parms! {
    /// `CPlate::Parms` (CPlate.h), the formation. File: `cunit.bin`.
    pub struct PlateParams {
        start_offset: f32 = 30.0, b"p00";
        length_limit: f32 = 150.0, b"p01";
        max_pos_size: f32 = 12.0, b"p02";
    }
}

parms! {
    /// `AIConstants::Parms` (AIConstant.h). File: `aiconst.bin`.
    pub struct AiParams {
        gravity: f32 = 550.0, b"p00";
        /// "ピキ：オートアタック" (`_34`).
        auto_attack: i32 = 1, b"p01";
        /// "攻撃したら戻る" (`_A4`): return to formation after an attack.
        return_after_attack: i32 = 1, b"p08";
        /// "１回だけ攻撃=1 死ぬまで攻撃=0" (`_C4`): 1 = one attack, 0 = keep attacking until the target dies.
        attack_once: i32 = 1, b"p10";
        do_scale_happa_move_speed: i32 = 1, b"p14";
        max_pikis_on_field: i32 = 100, b"p15";
    }
}

/// Everything the sim reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub navi_creature: CreatureParams,
    pub navi: NaviParams,
    pub piki_creature: CreatureParams,
    pub piki: PikiParams,
    pub plate: PlateParams,
    pub ai: AiParams,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            // NaviProp::NaviProp (navi.cpp)
            navi_creature: CreatureParams {
                friction: 1.0,
                face_dir_adjust: 0.5,
                acceleration: 0.2,
                ..Default::default()
            },
            navi: NaviParams::default(),
            // PikiProp::PikiProp (PikiMgr.h)
            piki_creature: CreatureParams {
                friction: 0.1,
                ..Default::default()
            },
            piki: PikiParams::default(),
            plate: PlateParams::default(),
            ai: AiParams::default(),
        }
    }
}

impl Params {
    /// `parms/naviMgr.bin`: CreatureProp block, then Navi block.
    pub fn apply_navi_mgr_bin(&mut self, data: &[u8]) -> Result<(), ParmError> {
        let mut pos = 0;
        self.navi_creature.read_block(data, &mut pos)?;
        self.navi.read_block(data, &mut pos)
    }

    /// `parms/pikiMgr.bin`: CreatureProp block, then Piki block.
    pub fn apply_piki_mgr_bin(&mut self, data: &[u8]) -> Result<(), ParmError> {
        let mut pos = 0;
        self.piki_creature.read_block(data, &mut pos)?;
        self.piki.read_block(data, &mut pos)
    }

    /// `parms/cunit.bin`
    pub fn apply_cunit_bin(&mut self, data: &[u8]) -> Result<(), ParmError> {
        self.plate.read_block(data, &mut 0)
    }

    /// `parms/aiconst.bin`
    pub fn apply_aiconst_bin(&mut self, data: &[u8]) -> Result<(), ParmError> {
        self.ai.read_block(data, &mut 0)
    }

    /// Loads whichever of the four files exist in `dir` (e.g. a folder extracted from the
    /// user's disc). Returns the names it applied.
    pub fn load_p1_dir(
        &mut self,
        dir: &std::path::Path,
    ) -> Result<Vec<&'static str>, Box<dyn std::error::Error>> {
        type Apply = fn(&mut Params, &[u8]) -> Result<(), ParmError>;
        let files: [(&str, Apply); 4] = [
            ("naviMgr.bin", Params::apply_navi_mgr_bin),
            ("pikiMgr.bin", Params::apply_piki_mgr_bin),
            ("cunit.bin", Params::apply_cunit_bin),
            ("aiconst.bin", Params::apply_aiconst_bin),
        ];
        let mut applied = Vec::new();
        for (name, apply) in files {
            let path = dir.join(name);
            if path.exists() {
                apply(self, &std::fs::read(&path)?)?;
                applied.push(name);
            }
        }
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &[u8; 3], value: [u8; 4]) -> Vec<u8> {
        let mut v = vec![id[0], id[1], id[2], 4];
        v.extend_from_slice(&value);
        v
    }

    #[test]
    fn reads_block_and_skips_unknown() {
        let mut data = Vec::new();
        data.extend(entry(b"p00", 17.5f32.to_be_bytes()));
        data.extend(entry(b"zzz", [1, 2, 3, 4]));
        data.extend(entry(b"p02", 7.0f32.to_be_bytes()));
        data.extend((-1i32).to_be_bytes());

        let mut p = Params::default();
        p.apply_cunit_bin(&data).unwrap();
        assert_eq!(p.plate.start_offset, 17.5);
        assert_eq!(p.plate.length_limit, 150.0);
        assert_eq!(p.plate.max_pos_size, 7.0);
    }

    #[test]
    fn duplicate_id_goes_to_first_field() {
        let mut data = Vec::new();
        data.extend((-1i32).to_be_bytes()); // empty CreatureProp block
        data.extend(entry(b"p48", 0.5f32.to_be_bytes()));
        data.extend((-1i32).to_be_bytes());

        let mut p = Params::default();
        p.apply_navi_mgr_bin(&data).unwrap();
        assert_eq!(p.navi.cursor_move_delay_time, 0.5);
        assert_eq!(p.navi.clamp_stick_threshold, 0.85);
    }

    #[test]
    fn truncated_is_an_error() {
        let mut p = Params::default();
        assert_eq!(p.apply_cunit_bin(b"p00\x04\x00"), Err(ParmError::Truncated));
    }

    /// Runs only when the user has extracted their disc next to the repo.
    #[test]
    fn user_disc_values_if_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../extracted/parms");
        if !dir.exists() {
            eprintln!("skipping: {} not found", dir.display());
            return;
        }
        let mut p = Params::default();
        let applied = p.load_p1_dir(&dir).unwrap();
        assert_eq!(applied.len(), 4);
        // Spot-check values known to differ from the code defaults.
        assert_eq!(p.navi.move_speed, 160.0);
        assert_eq!(p.navi.whistle_max_radius, 90.0);
        assert_eq!(p.piki.max_leaf_move_speed, 120.0);
        assert_eq!(p.ai.gravity, 560.0);
    }
}
