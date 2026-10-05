//! Loads Pikmin 1 models, textures and animations from the user's own disc image.
//!
//! Nothing here ships Nintendo data: every asset is read at runtime from a disc the user
//! provides. Formats are reimplemented from the projectPiki decomp (file/function citations
//! are in each module). The crate is renderer-agnostic: hosts get triangle batches, decoded
//! RGBA images and posed joint matrices, and draw them however they like.

pub mod anim;
pub mod disc;
pub mod model;
pub mod motion;
pub mod pose;
mod reader;
pub mod texture;

use std::fmt;

pub use anim::{Anim, AnimInfo, AnimMgr};
pub use disc::Disc;
pub use model::{Batch, BatchVertex, Blend, Material, Model, Skin};
pub use pose::{Pose, Skeleton};
pub use texture::Image;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Truncated(usize),
    Bad(String),
    NotFound(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Truncated(at) => write!(f, "data ended early at {at:#x}"),
            Error::Bad(s) => write!(f, "{s}"),
            Error::NotFound(p) => write!(f, "{p} is not on the disc"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Disc paths under `files/`.
pub mod paths {
    /// Olimar (`NaviMgr::NaviMgr`, naviMgr.cpp).
    pub const OLIMAR: &str = "dataDir/pikis/nv3Model.mod";
    /// Pikmin by `PikiColor` order Blue, Red, Yellow (`ViewPiki`/`PikiShapeObject`, viewPiki.cpp).
    pub const PIKMIN: [&str; 3] = [
        "dataDir/pikis/bluModel.mod",
        "dataDir/pikis/redModel.mod",
        "dataDir/pikis/yelModel.mod",
    ];
    /// The animation bundle every Pikmin and Olimar share (gameSetup.cpp).
    pub const PIKI_BUNDLE: &str = "dataDir/pikis/bluModel.anm";
    pub const PIKI_ANIM_MGR: &str = "dataDir/pikis/animMgr.bin";
    /// Leaf, bud, flower, drawn on joint 6 (`ViewPiki::demoDraw`, viewPiki.cpp).
    pub const HAPPA: [&str; 3] = [
        "dataDir/pikis/happas/leaf.mod",
        "dataDir/pikis/happas/bud.mod",
        "dataDir/pikis/happas/flower.mod",
    ];
    pub const ONION: &str = "dataDir/objects/goal/goal.mod";
    pub const ONION_BUNDLE: &str = "dataDir/objects/goal/goal.anm";
    pub const ONION_ANIM_MGR: &str = "dataDir/objects/goal/anims.bin";
    /// Numbered pellets 1, 5, 10, 20 (gameSetup.cpp).
    pub const PELLETS: [&str; 4] = [
        "dataDir/objects/pellets/white1.mod",
        "dataDir/objects/pellets/white2.mod",
        "dataDir/objects/pellets/white3.mod",
        "dataDir/objects/pellets/white4.mod",
    ];
}

/// An animation set: the AnimMgr's infos with their data resolved from a bundle.
#[derive(Debug, Clone)]
pub struct AnimSet {
    pub mgr: AnimMgr,
    /// `anims[i]` is the data for `mgr.infos[i]`, if the bundle had it.
    pub anims: Vec<Option<Anim>>,
}

impl AnimSet {
    /// Matches each AnimInfo to the bundle entry at `base_path/name`
    /// (`AnimMgr::loadAnims`, animMgr.cpp). Comparison ignores case and slash style.
    pub fn new(mgr: AnimMgr, bundle: Vec<Anim>) -> Self {
        let norm = |s: &str| s.replace('\\', "/").to_ascii_lowercase();
        let anims = mgr
            .infos
            .iter()
            .map(|info| {
                let want = norm(&format!("{}/{}", mgr.base_path, info.name));
                bundle
                    .iter()
                    .find(|a| {
                        let have = norm(&a.name);
                        have == want || have.ends_with(&format!("/{}", norm(&info.name)))
                    })
                    .cloned()
            })
            .collect();
        AnimSet { mgr, anims }
    }

    pub fn load(disc: &Disc, anim_mgr: &str, bundle: &str) -> Result<Self, Error> {
        let mgr = AnimMgr::parse(&disc.read(anim_mgr)?)?;
        let bundle = anim::parse_bundle(&disc.read(bundle)?)?;
        Ok(Self::new(mgr, bundle))
    }

    pub fn get(&self, index: usize) -> Option<(&AnimInfo, &Anim)> {
        Some((self.mgr.infos.get(index)?, self.anims.get(index)?.as_ref()?))
    }
}

/// Default disc location for development: the decomp's `orig/` folder next to the repo.
pub fn find_disc() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("PIKMIN_DISC") {
        return Some(p.into());
    }
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in ["../../../pikmin-main/orig/GPIE01_01", "../../../pikmin-main/pikmin-main/orig/GPIE01_01"] {
        let dir = here.join(dir);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if matches!(ext.as_str(), "iso" | "ciso" | "gcm") {
                    return Some(p);
                }
            }
        }
    }
    None
}
