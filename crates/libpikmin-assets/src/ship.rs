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
