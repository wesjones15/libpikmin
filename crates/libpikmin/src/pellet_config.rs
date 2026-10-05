//! Pellet configs from the disc's `parms/pelMgr.bin`: carrier counts, seeds and sizes for
//! numbered pellets, enemy corpses and ship parts.
//!
//! Format (`PelletMgr::readConfigs`, `PelletConfig::read`, pelletMgr.cpp): a count, then per
//! config a `Parameters` block (ids from `PelletConfig::PelletConfig`), then three `ID32`s
//! (model, pellet, unused) and the repair-animation joint. A corpse's config is the one whose
//! pellet id is the teki's type id (`BTeki::dieSoon` → `becomePellet`, tekibteki.cpp).

use crate::params::ParmError;
use crate::piki::PikiColor;

#[derive(Debug, Clone, PartialEq)]
pub struct PelletConfig {
    /// `x99` "theName"
    pub name: String,
    /// `p00`: 0-2 = pellet, 3 = UFO part.
    pub kind: i32,
    /// `p09`: `PELCOLOR_*` (0 blue, 1 red, 2 yellow; anything else none).
    pub color: Option<PikiColor>,
    /// `p01` / `p02`
    pub min_carriers: i32,
    pub max_carriers: i32,
    /// `p06` / `p07`: seeds into a matching / other Onion (negative = fireworks).
    pub seeds_matching: i32,
    pub seeds_other: i32,
    /// `p10`
    pub scale: f32,
    /// `p11`: height of the carrier-count display.
    pub carry_info_height: f32,
    /// Four-character ids, e.g. `"tksw"` (Spotty Bulborb corpse) or `"pb01"` (blue 1 pellet).
    /// A corpse uses the config whose `model_id` is its teki type id.
    pub model_id: String,
    pub pellet_id: String,
}

struct R<'a> {
    d: &'a [u8],
    o: usize,
}

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], ParmError> {
        let s = self.d.get(self.o..self.o + n).ok_or(ParmError::Truncated)?;
        self.o += n;
        Ok(s)
    }
    fn i32(&mut self) -> Result<i32, ParmError> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    /// `ID32::read` stores the four bytes reversed, so the readable id is the file bytes
    /// backwards (`"wskt"` on disc is `tksw`, teki swallow).
    fn id(&mut self) -> Result<String, ParmError> {
        let mut b = self.take(4)?.to_vec();
        b.reverse();
        Ok(String::from_utf8_lossy(&b).into_owned())
    }
}

/// Parses `pelMgr.bin`.
pub fn parse_pellet_configs(data: &[u8]) -> Result<Vec<PelletConfig>, ParmError> {
    let mut r = R { d: data, o: 0 };
    let n = r.i32()?.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let mut c = PelletConfig {
            name: String::new(),
            kind: 0,
            color: None,
            min_carriers: 1,
            max_carriers: 1,
            seeds_matching: 1,
            seeds_other: 1,
            scale: 1.0,
            carry_info_height: 30.0,
            model_id: String::new(),
            pellet_id: String::new(),
        };
        loop {
            let h = r.i32()?;
            if h == -1 {
                break;
            }
            let b = h.to_be_bytes();
            let id = [b[0], b[1], b[2]];
            if &id == b"x99" {
                let len = r.i32()?.max(0) as usize;
                let s = r.take(len)?;
                let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                c.name = String::from_utf8_lossy(&s[..end]).into_owned();
                continue;
            }
            let raw = r.take(b[3] as usize)?;
            let Ok(word): Result<[u8; 4], _> = raw.try_into() else {
                continue;
            };
            let i = i32::from_be_bytes(word);
            let f = f32::from_be_bytes(word);
            match &id {
                b"p00" => c.kind = i,
                b"p09" => {
                    c.color = match i {
                        0 => Some(PikiColor::Blue),
                        1 => Some(PikiColor::Red),
                        2 => Some(PikiColor::Yellow),
                        _ => None,
                    }
                }
                b"p01" => c.min_carriers = i,
                b"p02" => c.max_carriers = i,
                b"p06" => c.seeds_matching = i,
                b"p07" => c.seeds_other = i,
                b"p10" => c.scale = f,
                b"p11" => c.carry_info_height = f,
                _ => {}
            }
        }
        c.model_id = r.id()?;
        c.pellet_id = r.id()?;
        r.id()?;
        r.i32()?;
        out.push(c);
    }
    Ok(out)
}

/// Finds a config by its model id (`PelletMgr::getConfig`).
pub fn find<'a>(configs: &'a [PelletConfig], id: &str) -> Option<&'a PelletConfig> {
    configs.iter().find(|c| c.model_id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_disc_pellets_if_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/parms/pelMgr.bin");
        let Ok(data) = std::fs::read(&path) else {
            eprintln!("skipping: {} not found", path.display());
            return;
        };
        let configs = parse_pellet_configs(&data).unwrap();
        for c in &configs {
            println!(
                "{:>14} model {:?} pellet {:?} kind {} color {:?} carriers {}-{} seeds {}/{} scale {} h {}",
                c.name, c.model_id, c.pellet_id, c.kind, c.color, c.min_carriers, c.max_carriers,
                c.seeds_matching, c.seeds_other, c.scale, c.carry_info_height
            );
        }
        assert!(configs.len() > 10);
    }
}
