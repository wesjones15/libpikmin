//! Pikmin 1's sound data (JAudio 1): the wave/instrument bank, AFC-compressed waves, the BMS
//! sequence archive and `.stx` streams. Playing them is `libpikmin-audio`'s job; this module
//! only parses and decodes.

pub mod adpcm;
pub mod bank;
pub mod barc;
pub mod stream;
pub mod tables;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use bank::{Ibnk, Inst, Osc, Perc, SoundBank, Wave, Wsys};
pub use barc::SeqArchive;
pub use stream::Stream;

use crate::{Assets, Error};

pub const BANK: &str = "SndData/Banks/pikibank.bx";
pub const SEQ_ARC: &str = "SndData/Seqs/pikiseq.arc";
pub const SEQ_HED: &str = crate::extract::SEQ_HEADER;

/// (physical wsys, archive, wave)
type WaveKey = (usize, usize, usize);

/// The bank plus lazily decoded waves.
pub struct SoundData {
    pub bank: SoundBank,
    pub seqs: SeqArchive,
    assets: Assets,
    files: Mutex<HashMap<String, Arc<Vec<u8>>>>,
    pcm: Mutex<HashMap<WaveKey, Arc<Vec<i16>>>>,
}

impl SoundData {
    pub fn load(assets: &Assets) -> Result<Self, Error> {
        Ok(SoundData {
            bank: SoundBank::parse(&assets.read(BANK)?)?,
            seqs: SeqArchive::parse(&assets.read(SEQ_HED)?, assets.read(SEQ_ARC)?)?,
            assets: assets.clone(),
            files: Mutex::default(),
            pcm: Mutex::default(),
        })
    }

    /// A wave decoded to 16-bit PCM (cached).
    pub fn pcm(&self, wsys: usize, archive: usize, wave: usize) -> Result<Arc<Vec<i16>>, Error> {
        if let Some(p) = self.pcm.lock().unwrap().get(&(wsys, archive, wave)) {
            return Ok(p.clone());
        }
        let ws = self
            .bank
            .wsys
            .get(wsys)
            .and_then(Option::as_ref)
            .ok_or(Error::Bad(format!("no wsys {wsys}")))?;
        let arc = ws
            .archives
            .get(archive)
            .ok_or(Error::Bad(format!("no archive {archive}")))?;
        let w = arc
            .waves
            .get(wave)
            .ok_or(Error::Bad(format!("no wave {wave}")))?;
        let file = {
            let mut files = self.files.lock().unwrap();
            match files.get(&arc.file) {
                Some(f) => f.clone(),
                None => {
                    let f = Arc::new(self.assets.read(&format!("SndData/Banks/{}", arc.file))?);
                    files.insert(arc.file.clone(), f.clone());
                    f
                }
            }
        };
        let start = w.offset as usize;
        let bytes = file
            .get(start..start + w.length as usize)
            .ok_or(Error::Truncated(start))?;
        let pcm = Arc::new(adpcm::decode_wave(w.format, bytes, w.end as usize));
        self.pcm
            .lock()
            .unwrap()
            .insert((wsys, archive, wave), pcm.clone());
        Ok(pcm)
    }
}
