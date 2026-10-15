//! First-run extraction: copies the files libpikmin needs off the user's own disc image into
//! the git-ignored `assets/` folder, with no external tools. After that the disc is never
//! read again.
//!
//! Pikmin 1 keeps these as loose files under `files/dataDir/`, so this is a straight copy
//! (no archives or compression). The folder layout matches `dataDir/`:
//!
//! - `parms/`: tuning `.bin` files
//! - `pikis/`: Olimar and Pikmin models, the shared animation bundle, `animMgr.bin`, leaves
//!   (includes OCARRY, Olimar's geyser pose, and ROLLJMP, Pikmin's airborne motion;
//!   these are bundle entries, not separate files on the disc)
//! - `objects/goal/`: the Onion model, animations and `anims.bin`
//! - `objects/pellets/`: the numbered pellets (1, 5, 10, 20)
//! - `objects/pikihead/`: the sprout / flying seed
//! - `objects/water/`, `objects/fl_water/`: nectar puddle and falling drop
//! - `tekis/<name>/`, `tekikeys/`, `tekipara/`: enemy models, animation keys and parameters
//!   (Dwarf Bulborb = chappy, Spotty Bulborb = swallow, Honeywisp = qurione)
//! - `SndData/`: sound banks, sequences and streamed music, plus `SndData/Seqs/pikiseq.hed`,
//!   the sequence archive's index, which the game keeps inside `main.dol` (`HEAD_pikiseq`,
//!   passed to `Jaf_InitSeqArchive2` in verysimple.c) and this extractor copies out.
//! - `effects/`: particle generators (`pcr/`), their textures (`tex/`) and effect models
//! - `shapes/cursor/`: the cursor and its marker (`GlobalShape::init`)
//! - `objects/ufo/`, `objects/ufoparts/`, `objects/pelAnim.bin`: the S.S. Dolphin, its parts and
//!   which model each part uses
//! - `screen/eng_blo/`, `screen/eng_tex/`, `bigFont.bti`: the English 2D screens (messages,
//!   ship part descriptions), their textures and the font

use std::path::{Component, Path, PathBuf};

use crate::{Disc, Error};

/// Folders under `files/dataDir/` copied whole.
pub const FOLDERS: [&str; 21] = [
    "parms/",
    "pikis/",
    "objects/goal/",
    "objects/pellets/",
    "objects/pikihead/",
    "objects/water/",
    "objects/fl_water/",
    "tekis/chappy/",
    "tekis/swallow/",
    "tekis/qurione/",
    "tekikeys/",
    "tekipara/",
    "SndData/",
    "effects/",
    "shapes/cursor/",
    "objects/ufo/",
    "objects/ufoparts/",
    "objects/pelAnim.bin",
    "screen/eng_blo/",
    "screen/eng_tex/",
    "bigFont.bti",
];

/// Files that must exist for the sim and viewer to work.
pub const REQUIRED: [&str; 32] = [
    "parms/naviMgr.bin",
    "parms/pikiMgr.bin",
    "parms/aiconst.bin",
    "pikis/animMgr.bin",
    "pikis/bluModel.anm",
    "pikis/bluModel.mod",
    "pikis/redModel.mod",
    "pikis/yelModel.mod",
    "pikis/nv3Model.mod",
    "pikis/happas/leaf.mod",
    "objects/goal/goal.mod",
    "objects/goal/goal.anm",
    "objects/goal/anims.bin",
    "objects/pellets/white1.mod",
    "objects/pikihead/pikihead.mod",
    "objects/water/water.mod",
    "objects/fl_water/fl_water.mod",
    "tekis/chappy/chappy.mod",
    "tekis/swallow/swallow.mod",
    "tekikeys/swallow.key",
    "tekipara/swallow.bin",
    "SndData/Banks/pikibank.bx",
    "effects/pcr/g_curs00.pcr",
    "shapes/cursor/cursor.mod",
    crate::ship::SHIP_MODEL,
    crate::ship::MESSAGE_BASE_SCREEN,
    "bigFont.bti",
    "SndData/Seqs/pikiseq.arc",
    SEQ_HEADER,
    crate::sound::tables::TABLES,
    crate::sound::tables::SOUND_TABLE,
    crate::sound::tables::ONION_BOOT,
];

/// The sequence archive index copied out of `main.dol`.
pub const SEQ_HEADER: &str = "SndData/Seqs/pikiseq.hed";

/// Finds the `BARC` index for `pikiseq.arc` in the executable: `"BARC----"`, the entry count
/// at 0x0C and the archive name at 0x10, then 32 bytes per entry (virload.c `JV_InitHeader_M`).
fn find_seq_header(dol: &[u8]) -> Option<&[u8]> {
    let mut at = 0;
    while let Some(i) = dol[at..].windows(8).position(|w| w == b"BARC----") {
        let start = at + i;
        let h = &dol[start..];
        if h.len() >= 0x20 && h[0x10..].starts_with(b"pikiseq.arc\0") {
            let count = u32::from_be_bytes(h[0xC..0x10].try_into().unwrap()) as usize;
            let len = 0x20 * (count + 1);
            return (count < 0x100 && h.len() >= len).then(|| &h[..len]);
        }
        at = start + 8;
    }
    None
}

pub const GAME_IDS: [(&str, &str); 3] = [
    ("GPIJ01", "Japan"),
    ("GPIE01", "USA"),
    ("GPIP01", "Europe (PAL)"),
];

/// Is `dir` a complete extraction?
pub fn is_complete(dir: &Path) -> bool {
    REQUIRED.iter().all(|r| dir.join(r).is_file())
}

/// A destination built from disc-supplied names must stay under the output folder.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, Error> {
    let p = Path::new(rel);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(Error::Bad(format!(
            "disc path {rel:?} escapes the output folder"
        )));
    }
    Ok(base.join(p))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub game_id: String,
    pub files: usize,
    pub out: PathBuf,
}

/// Extracts everything into `out` from the disc image at `iso`. Does nothing if `out` is
/// already complete. `log` receives short progress messages.
pub fn extract_assets(iso: &Path, out: &Path, log: &mut dyn FnMut(&str)) -> Result<Report, Error> {
    let disc = Disc::open(iso)?;
    let Some((_, region)) = GAME_IDS.iter().find(|(id, _)| *id == disc.game_id) else {
        return Err(Error::Bad(format!(
            "this disc is {}, not Pikmin (GPIJ01/GPIE01/GPIP01)",
            disc.game_id
        )));
    };
    log(&format!("disc: {} {region}", disc.game_id));
    if is_complete(out) {
        log("assets already complete; nothing to do");
        return Ok(Report {
            game_id: disc.game_id,
            files: 0,
            out: out.to_path_buf(),
        });
    }

    let mut files: Vec<&str> = disc.paths().collect();
    files.sort_unstable();
    let mut total = 0;
    for folder in FOLDERS {
        let prefix = format!("dataDir/{folder}").to_ascii_lowercase();
        let mut n = 0;
        for &path in &files {
            if !path.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }
            let rel = &path["dataDir/".len()..];
            let dest = safe_join(out, rel)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(dest, disc.read(path)?)?;
            n += 1;
        }
        if n == 0 {
            return Err(Error::NotFound(format!("dataDir/{folder}").into()));
        }
        log(&format!("dataDir/{folder} ({n} files)"));
        total += n;
    }

    let dol = disc.read_dol()?;
    let hed = find_seq_header(&dol)
        .ok_or_else(|| Error::Bad("no pikiseq BARC header in main.dol".into()))?;
    std::fs::create_dir_all(out.join("SndData/Seqs"))?;
    std::fs::write(out.join(SEQ_HEADER), hed)?;
    log(&format!(
        "{SEQ_HEADER} from main.dol ({} sequences)",
        hed.len() / 0x20 - 1
    ));
    let (tables, names) = crate::sound::tables::extract(&dol, &disc.game_id, disc.version)?;
    std::fs::write(out.join(crate::sound::tables::TABLES), tables)?;
    std::fs::write(out.join(crate::sound::tables::SOUND_TABLE), names)?;
    std::fs::write(out.join(crate::sound::tables::ONION_BOOT),
        crate::sound::tables::extract_onion_boot(&dol, &disc.game_id, disc.version)?)?;
    log(&format!(
        "{} and {} from main.dol",
        crate::sound::tables::TABLES,
        crate::sound::tables::SOUND_TABLE
    ));
    total += 4;

    if let Some(missing) = REQUIRED.iter().find(|r| !out.join(r).is_file()) {
        return Err(Error::NotFound(out.join(missing)));
    }
    std::fs::write(
        out.join("SOURCE.txt"),
        format!(
            "extracted from: {}\ngame id: {}\nfiles: {total}\nby: libpikmin_assets::extract\n",
            iso.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
            disc.game_id,
        ),
    )?;
    log(&format!("done: {total} files"));
    Ok(Report {
        game_id: disc.game_id,
        files: total,
        out: out.to_path_buf(),
    })
}

/// Looks for a Pikmin disc image: `PIKMIN_DISC`, then `.iso`/`.ciso`/`.gcm` files in `dirs`.
/// Each candidate is opened and only a Pikmin 1 game id is accepted; names containing
/// "pikmin" are tried first.
pub fn find_disc(dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PIKMIN_DISC") {
        return Some(p.into());
    }
    let mut found: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()))
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            matches!(ext.to_ascii_lowercase().as_str(), "iso" | "ciso" | "gcm")
        })
        .collect();
    let score = |p: &PathBuf| {
        let n = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        !(n.contains("pikmin") && !n.contains('2'))
    };
    found.sort_by_key(score);
    found
        .into_iter()
        .find(|p| Disc::open(p).is_ok_and(|d| GAME_IDS.iter().any(|(id, _)| *id == d.game_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_traversal_is_rejected() {
        assert!(safe_join(Path::new("out"), "../evil").is_err());
        assert!(safe_join(Path::new("out"), "/abs").is_err());
        assert!(safe_join(Path::new("out"), "pikis/nv3Model.mod").is_ok());
    }
}
