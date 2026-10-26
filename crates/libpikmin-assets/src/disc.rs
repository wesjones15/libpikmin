//! Reads files off the user's GameCube disc image. Only the first-run extractor
//! ([`crate::extract`]) uses it. Supports plain `.iso`/`.gcm` and `.ciso` (compact ISO).
//!
//! The image, CISO and file system table handling is `libcommon::disc`, shared with the other
//! GameCube libraries. This adds Pikmin's lookups: paths relative to the disc's `files/` root,
//! matched without regard to case or slash direction.

use std::collections::HashMap;
use std::path::Path;

use libcommon::disc::{Disc as Image, DiscError};

use crate::Error;

impl From<DiscError> for Error {
    fn from(e: DiscError) -> Self {
        match e {
            DiscError::Io(e) => Error::Io(e),
            DiscError::Missing(p) => Error::NotFound(p.into()),
            e => Error::Bad(e.to_string()),
        }
    }
}

pub struct Disc {
    image: Image,
    /// Game ID from the disc header, e.g. `GPIE01`.
    pub game_id: String,
    /// Disc revision (header byte 7), e.g. 1 for "Rev 1".
    pub version: u8,
    /// Normalized path to index in the image's file list.
    by_path: HashMap<String, usize>,
}

impl Disc {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let image = Image::open(path)?;
        let by_path = image
            .files()
            .iter()
            .enumerate()
            .map(|(i, f)| (normalize(&f.path), i))
            .collect();
        Ok(Disc { game_id: image.game_id.clone(), version: image.version, image, by_path })
    }

    fn entry(&self, path: &str) -> Option<&libcommon::disc::DiscFile> {
        let key = normalize(path.strip_prefix("files/").unwrap_or(path));
        self.by_path.get(&key).map(|&i| &self.image.files()[i])
    }

    /// Reads a file by its path under the disc's `files/` root, e.g.
    /// `dataDir/pikis/nv3Model.mod` (a leading `files/` is accepted too).
    pub fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        let e = self.entry(path).ok_or_else(|| Error::NotFound(path.into()))?;
        Ok(self.image.read(e)?)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.entry(path).is_some()
    }

    /// The game executable (`sys/main.dol`).
    pub fn read_dol(&self) -> Result<Vec<u8>, Error> {
        Ok(self.image.read_dol()?)
    }

    /// All file paths on the disc, spelled as on the disc.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.image.files().iter().map(|f| f.path.as_str())
    }
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}
