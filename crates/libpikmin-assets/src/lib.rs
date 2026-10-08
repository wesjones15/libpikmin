//! Loads Pikmin 1 models, textures and animations extracted from the user's own disc.
//!
//! Nothing here ships Nintendo data. On first run [`extract`] copies the needed folders off the
//! user's disc image into the git-ignored `assets/` folder (same layout as the disc's
//! `files/dataDir`), and everything loads from there. Formats are reimplemented from the
//! projectPiki decomp (file/function citations are in each module). The crate is
//! renderer-agnostic: hosts get triangle batches, decoded RGBA images and posed joint matrices.

pub mod anim;
pub mod disc;
pub mod extract;
pub mod model;
pub mod motion;
pub mod pani;
pub mod particle;
pub mod pose;
pub mod screen;
pub mod ship;
pub mod sound;
pub mod teki;
mod reader;
pub mod texture;

use std::fmt;

pub use anim::{Anim, AnimInfo, AnimMgr};
pub use disc::Disc;
pub use model::{Batch, BatchVertex, Blend, CollSphere, Cull, Material, Model, Skin};
pub use pose::{Pose, Skeleton};
pub use texture::Image;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Truncated(usize),
    Bad(String),
    NotFound(std::path::PathBuf),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Truncated(at) => write!(f, "data ended early at {at:#x}"),
            Error::Bad(s) => write!(f, "{s}"),
            Error::NotFound(p) => write!(
                f,
                "{} is missing; point PIKMIN_DISC at your Pikmin disc image to extract assets",
                p.display()
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Paths inside the assets folder (= under the disc's `files/dataDir/`).
pub mod paths {
    /// Olimar (`NaviMgr::NaviMgr`, naviMgr.cpp).
    pub const OLIMAR: &str = "pikis/nv3Model.mod";
    /// Pikmin by `PikiColor` order Blue, Red, Yellow (`ViewPiki`/`PikiShapeObject`, viewPiki.cpp).
    pub const PIKMIN: [&str; 3] = [
        "pikis/bluModel.mod",
        "pikis/redModel.mod",
        "pikis/yelModel.mod",
    ];
    /// The animation bundle every Pikmin and Olimar share (gameSetup.cpp).
    pub const PIKI_BUNDLE: &str = "pikis/bluModel.anm";
    pub const PIKI_ANIM_MGR: &str = "pikis/animMgr.bin";
    /// Leaf, bud, flower, drawn on joint 6 (`ViewPiki::demoDraw`, viewPiki.cpp).
    pub const HAPPA: [&str; 3] = [
        "pikis/happas/leaf.mod",
        "pikis/happas/bud.mod",
        "pikis/happas/flower.mod",
    ];
    pub const ONION: &str = "objects/goal/goal.mod";
    pub const ONION_BUNDLE: &str = "objects/goal/goal.anm";
    pub const ONION_ANIM_MGR: &str = "objects/goal/anims.bin";
    /// The Onions' light cones (`EffectMgr::MOD_{Blue,Red,Yellow}Onyon`, effectMgr.cpp's
    /// `geometryLoadInfo`, drawn at 0.1 scale), by `PikiColor`.
    pub const ONION_BEAMS: [&str; 3] = [
        "effects/shapes/b_goal.mod",
        "effects/shapes/r_goal.mod",
        "effects/shapes/y_goal.mod",
    ];
    /// Numbered pellets 1, 5, 10, 20 (gameSetup.cpp).
    pub const PELLETS: [&str; 4] = [
        "objects/pellets/white1.mod",
        "objects/pellets/white2.mod",
        "objects/pellets/white3.mod",
        "objects/pellets/white4.mod",
    ];
    /// The sprout / flying seed (`ItemMgr`, itemMgr.cpp); its leaf goes on joint 3.
    pub const PIKIHEAD: &str = "objects/pikihead/pikihead.mod";
    pub const PIKIHEAD_BUNDLE: &str = "objects/pikihead/pikihead.anm";
    pub const PIKIHEAD_ANIM_MGR: &str = "objects/pikihead/anims.bin";
    /// Nectar puddle (`OBJTYPE_Water`) and falling drop (`OBJTYPE_FallWater`).
    pub const WATER: &str = "objects/water/water.mod";
    pub const WATER_BUNDLE: &str = "objects/water/water.anm";
    pub const WATER_ANIM_MGR: &str = "objects/water/anims.bin";
    pub const FALL_WATER: &str = "objects/fl_water/fl_water.mod";
    pub const FALL_WATER_BUNDLE: &str = "objects/fl_water/fl_water.anm";
    pub const FALL_WATER_ANIM_MGR: &str = "objects/fl_water/anims.bin";
    /// The cursor and the marker tinted with the next Pikmin's colour (`GlobalShape::init`,
    /// drawn by `Navi::refresh`).
    pub const CURSOR: &str = "shapes/cursor/cursor.mod";
    pub const CURSOR_MARKER: &str = "shapes/cursor/marker.mod";
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

    pub fn load(assets: &Assets, anim_mgr: &str, bundle: &str) -> Result<Self, Error> {
        let mgr = AnimMgr::parse(&assets.read(anim_mgr)?)?;
        let bundle = anim::parse_bundle(&assets.read(bundle)?)?;
        Ok(Self::new(mgr, bundle))
    }

    pub fn get(&self, index: usize) -> Option<(&AnimInfo, &Anim)> {
        Some((self.mgr.infos.get(index)?, self.anims.get(index)?.as_ref()?))
    }
}

/// The extracted assets folder.
#[derive(Debug, Clone)]
pub struct Assets {
    pub root: std::path::PathBuf,
}

impl Assets {
    /// `assets/` at the repository root.
    pub fn default_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    /// Uses `explicit`, else `PIKMIN_ASSETS`, else [`Assets::default_dir`]. Returns `None` if
    /// the folder hasn't been extracted yet.
    pub fn discover(explicit: Option<&std::path::Path>) -> Option<Self> {
        let candidates = [
            explicit.map(std::path::Path::to_path_buf),
            std::env::var_os("PIKMIN_ASSETS").map(Into::into),
            Some(Self::default_dir()),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|p| extract::is_complete(p))
            .map(|root| Assets { root })
    }

    /// First-run helper: if `dir` is not a complete extraction, build it from the disc image
    /// at `iso`, then use it. The disc is not touched again afterwards.
    pub fn ensure(iso: &std::path::Path, dir: &std::path::Path, log: &mut dyn FnMut(&str)) -> Result<Self, Error> {
        if !extract::is_complete(dir) {
            extract::extract_assets(iso, dir, log)?;
        }
        Ok(Assets { root: dir.to_path_buf() })
    }

    /// What a host calls at startup: an existing extraction if there is one, else extract into
    /// [`Assets::default_dir`] from a disc found by [`extract::find_disc`] in `disc_dirs`.
    pub fn discover_or_extract(disc_dirs: &[std::path::PathBuf], log: &mut dyn FnMut(&str)) -> Result<Self, Error> {
        if let Some(a) = Self::discover(None) {
            return Ok(a);
        }
        let iso = extract::find_disc(disc_dirs).ok_or_else(|| {
            Error::Bad("no assets yet and no Pikmin disc found; set PIKMIN_DISC to your .iso/.ciso".into())
        })?;
        log(&format!("first run: extracting assets from {}", iso.display()));
        Self::ensure(&iso, &Self::default_dir(), log)
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        let full = self.root.join(path);
        std::fs::read(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(full),
            _ => Error::Io(e),
        })
    }

    pub fn model(&self, path: &str) -> Result<Model, Error> {
        Model::parse(&self.read(path)?)
    }
}
