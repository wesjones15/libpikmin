//! Reads files straight off the user's GameCube disc image, so nothing has to be extracted
//! by hand. Supports plain `.iso`/`.gcm` and `.ciso` (compact ISO).
//!
//! GCM layout: the disc header holds the FST offset and size at 0x424/0x428. FST entries are
//! 12 bytes: `flags:u8 | name_offset:u24`, then `offset`, `size` (for a directory: parent
//! index and the index one past its last child). Entry 0 is the root and its size is the
//! entry count. Names follow the entry table.
//!
//! CISO: a 0x8000-byte header (`"CISO"`, little-endian block size, then one byte per block,
//! 1 = stored), followed by the stored blocks in order. Missing blocks read as zeros.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

use crate::Error;

const CISO_HEADER: u64 = 0x8000;

enum Layout {
    Plain,
    Ciso {
        block_size: u64,
        /// Index of each disc block within the file's stored blocks, `None` = all zeros.
        map: Vec<Option<u64>>,
    },
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    offset: u32,
    size: u32,
}

pub struct Disc {
    file: Mutex<File>,
    layout: Layout,
    /// Game ID from the disc header, e.g. `GPIE01`.
    pub game_id: String,
    files: HashMap<String, Entry>,
}

impl Disc {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let mut file = File::open(path)?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        let layout = if &magic[..4] == b"CISO" {
            let block_size = u32::from_le_bytes(magic[4..8].try_into().unwrap()) as u64;
            let mut flags = vec![0u8; (CISO_HEADER - 8) as usize];
            file.read_exact(&mut flags)?;
            let mut next = 0u64;
            let map = flags
                .iter()
                .map(|&f| {
                    (f != 0).then(|| {
                        next += 1;
                        next - 1
                    })
                })
                .collect();
            Layout::Ciso { block_size, map }
        } else {
            Layout::Plain
        };
        let mut disc = Disc {
            file: Mutex::new(file),
            layout,
            game_id: String::new(),
            files: HashMap::new(),
        };
        let header = disc.read_at(0, 0x440)?;
        disc.game_id = String::from_utf8_lossy(&header[..6]).into_owned();
        let be = |o: usize| u32::from_be_bytes(header[o..o + 4].try_into().unwrap());
        let (fst_off, fst_size) = (be(0x424), be(0x428));
        let fst = disc.read_at(fst_off as u64, fst_size as usize)?;
        disc.files = parse_fst(&fst)?;
        Ok(disc)
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, Error> {
        let mut out = vec![0u8; len];
        let mut file = self.file.lock().unwrap();
        match &self.layout {
            Layout::Plain => {
                file.seek(SeekFrom::Start(offset))?;
                file.read_exact(&mut out)?;
            }
            Layout::Ciso { block_size, map } => {
                let mut done = 0usize;
                while done < len {
                    let pos = offset + done as u64;
                    let (block, within) = (pos / block_size, pos % block_size);
                    let n = ((block_size - within) as usize).min(len - done);
                    if let Some(Some(stored)) = map.get(block as usize) {
                        file.seek(SeekFrom::Start(CISO_HEADER + stored * block_size + within))?;
                        file.read_exact(&mut out[done..done + n])?;
                    }
                    done += n;
                }
            }
        }
        Ok(out)
    }

    /// Reads a file by its path under the disc's `files/` root, e.g.
    /// `dataDir/pikis/nv3Model.mod` (a leading `files/` is accepted too).
    pub fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        let key = normalize(path.strip_prefix("files/").unwrap_or(path));
        let e = self
            .files
            .get(&key)
            .ok_or_else(|| Error::NotFound(path.to_string()))?;
        self.read_at(e.offset as u64, e.size as usize)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files
            .contains_key(&normalize(path.strip_prefix("files/").unwrap_or(path)))
    }

    /// All file paths on the disc, lowercase.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

fn parse_fst(fst: &[u8]) -> Result<HashMap<String, Entry>, Error> {
    let be = |o: usize| -> Result<u32, Error> {
        fst.get(o..o + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .ok_or(Error::Truncated(o))
    };
    let count = be(8)? as usize;
    let names = count * 12;
    let name_at = |off: usize| -> String {
        let s = &fst[(names + off).min(fst.len())..];
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        // Pikmin's file names are ASCII except a few Shift-JIS parm backups; lossy is fine.
        String::from_utf8_lossy(&s[..end]).into_owned()
    };

    let mut files = HashMap::new();
    // Stack of (end index, path prefix) for the directories we're inside.
    let mut dirs: Vec<(usize, String)> = vec![(count, String::new())];
    let mut i = 1;
    while i < count {
        while dirs.last().is_some_and(|(end, _)| i >= *end) {
            dirs.pop();
        }
        let word = be(i * 12)?;
        let (is_dir, name) = (word >> 24 != 0, name_at((word & 0xFF_FFFF) as usize));
        let prefix = dirs.last().map(|(_, p)| p.clone()).unwrap_or_default();
        let path = format!("{prefix}{name}");
        if is_dir {
            dirs.push((be(i * 12 + 8)? as usize, format!("{path}/")));
        } else {
            files.insert(
                path.to_ascii_lowercase(),
                Entry {
                    offset: be(i * 12 + 4)?,
                    size: be(i * 12 + 8)?,
                },
            );
        }
        i += 1;
    }
    Ok(files)
}
