//! The sequence archive: `Seqs/pikiseq.arc` (raw BMS sequences back to back) indexed by
//! `pikiseq.hed` (`BarcHeader` + `BarcEntry`s, pikiseq.h), which the extractor copies out of
//! `main.dol`.
//!
//! Sequence `n` is entry `n + 1` (entry 0 is the header). An entry whose `isDummy` isn't 0xFFFF
//! redirects to sequence `isDummy` (`JV_GetRealHandle`, virload.c). The ids are the `BGM_*`
//! enum (piki_bgm.h): 0 = pikise.jam (sound effects), 1 = sysevent.jam, 4 = tutorial.jam, ...

use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeqEntry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct SeqArchive {
    /// Index by sequence id, redirects resolved; `None` for broken redirects.
    pub entries: Vec<Option<SeqEntry>>,
    pub data: Vec<u8>,
}

impl SeqArchive {
    pub fn parse(hed: &[u8], arc: Vec<u8>) -> Result<Self, Error> {
        if hed.len() < 0x20 || &hed[..8] != b"BARC----" {
            return Err(Error::Bad("pikiseq.hed is not a BARC header".into()));
        }
        let u32_at = |o: usize| u32::from_be_bytes(hed[o..o + 4].try_into().unwrap());
        let count = u32_at(0xC) as usize;
        if hed.len() < 0x20 * (count + 1) {
            return Err(Error::Truncated(hed.len()));
        }
        let raw = |n: usize| {
            let e = 0x20 * (n + 1);
            let name = &hed[e..e + 14];
            let end = name.iter().position(|&b| b == 0).unwrap_or(14);
            let redirect = u16::from_be_bytes([hed[e + 0xE], hed[e + 0xF]]);
            (String::from_utf8_lossy(&name[..end]).into_owned(), redirect, u32_at(e + 0x18), u32_at(e + 0x1C))
        };
        let entries = (0..count)
            .map(|n| {
                let mut at = n;
                // `JV_GetRealHandle` follows redirects; cap the chain in case of a loop.
                for _ in 0..count {
                    let (name, redirect, offset, size) = raw(at);
                    if redirect == 0xFFFF {
                        let ok = (offset as usize).checked_add(size as usize).is_some_and(|e| e <= arc.len());
                        return ok.then_some(SeqEntry { name, offset, size });
                    }
                    if redirect as usize >= count {
                        return None;
                    }
                    at = redirect as usize;
                }
                None
            })
            .collect();
        Ok(SeqArchive { entries, data: arc })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The sequence's bytes.
    pub fn seq(&self, id: usize) -> Option<&[u8]> {
        let e = self.entries.get(id)?.as_ref()?;
        self.data.get(e.offset as usize..(e.offset + e.size) as usize)
    }

    pub fn name(&self, id: usize) -> Option<&str> {
        Some(self.entries.get(id)?.as_ref()?.name.as_str())
    }
}
