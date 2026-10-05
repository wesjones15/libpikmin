//! GameCube texture decoding to RGBA8.
//!
//! `TexImgFormat` (Texture.h) numbers the formats differently from GX; `Texture::decodeData`
//! (sysDolphin/texture.cpp) and `TexImg::getTileSize` (graphics.cpp) give the mapping and
//! tile sizes. The pixel layouts are the standard GX tiled formats.

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexFormat {
    Rgb565,
    /// GX CMPR (DXT1 in 8x8 tiles of four 4x4 blocks).
    S3tc,
    Rgb5a3,
    I4,
    I8,
    Ia4,
    Ia8,
    Rgba8,
    Z8,
}

impl TexFormat {
    /// `TexImgFormat` value as stored in MOD texture chunks.
    pub fn from_tex_img(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Rgb565,
            1 => Self::S3tc,
            2 => Self::Rgb5a3,
            3 => Self::I4,
            4 => Self::I8,
            5 => Self::Ia4,
            6 => Self::Ia8,
            7 => Self::Rgba8,
            8 => Self::Z8,
            _ => return None,
        })
    }

    /// Tile size in pixels.
    fn tile(self) -> (usize, usize) {
        match self {
            Self::I4 | Self::S3tc => (8, 8),
            Self::I8 | Self::Ia4 | Self::Z8 => (8, 4),
            _ => (4, 4),
        }
    }

    /// Bytes per tile.
    fn tile_bytes(self) -> usize {
        match self {
            Self::Rgba8 => 64,
            _ => 32,
        }
    }
}

/// A decoded image, row-major RGBA8, top row first.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn rgb565(c: u16) -> [u8; 4] {
    let r = ((c >> 11) & 0x1F) as u8;
    let g = ((c >> 5) & 0x3F) as u8;
    let b = (c & 0x1F) as u8;
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2), 255]
}

fn rgb5a3(c: u16) -> [u8; 4] {
    if c & 0x8000 != 0 {
        let r = ((c >> 10) & 0x1F) as u8;
        let g = ((c >> 5) & 0x1F) as u8;
        let b = (c & 0x1F) as u8;
        [(r << 3) | (r >> 2), (g << 3) | (g >> 2), (b << 3) | (b >> 2), 255]
    } else {
        let a = ((c >> 12) & 0x7) as u8;
        let r = ((c >> 8) & 0xF) as u8;
        let g = ((c >> 4) & 0xF) as u8;
        let b = (c & 0xF) as u8;
        [r * 0x11, g * 0x11, b * 0x11, (a << 5) | (a << 2) | (a >> 1)]
    }
}

/// A `.bti` texture (`TexImg::importBti`, graphics.cpp): the 0x20-byte `BtiHeader`, then the
/// image. Returns the image and whether S and T repeat (`!mWrapS` clamps).
pub fn decode_bti(data: &[u8]) -> Result<(Image, bool, bool), Error> {
    if data.len() < 0x20 {
        return Err(Error::Truncated(data.len()));
    }
    // `TexImg::convFormat`: GX texture formats; paletted ones aren't used for these files.
    let format = match data[0] {
        0 => TexFormat::I4,
        1 => TexFormat::I8,
        2 => TexFormat::Ia4,
        3 => TexFormat::Ia8,
        4 => TexFormat::Rgb565,
        5 => TexFormat::Rgb5a3,
        6 => TexFormat::Rgba8,
        14 => TexFormat::S3tc,
        f => return Err(Error::Bad(format!("bti format {f} is not supported"))),
    };
    let width = u16::from_be_bytes([data[2], data[3]]) as u32;
    let height = u16::from_be_bytes([data[4], data[5]]) as u32;
    let offset = u32::from_be_bytes(data[0x1C..0x20].try_into().unwrap()) as usize;
    let image = decode(format, width, height, data.get(offset..).unwrap_or(&[]))?;
    Ok((image, data[6] != 0, data[7] != 0))
}

/// Decodes the first image level.
pub fn decode(format: TexFormat, width: u32, height: u32, data: &[u8]) -> Result<Image, Error> {
    let (w, h) = (width as usize, height as usize);
    let (tw, th) = format.tile();
    let tiles_x = w.div_ceil(tw);
    let tiles_y = h.div_ceil(th);
    let need = tiles_x * tiles_y * format.tile_bytes();
    if data.len() < need {
        return Err(Error::Bad(format!(
            "{format:?} {w}x{h} needs {need} bytes, have {}",
            data.len()
        )));
    }

    let mut rgba = vec![0u8; w * h * 4];
    let mut put = |x: usize, y: usize, px: [u8; 4]| {
        if x < w && y < h {
            rgba[(y * w + x) * 4..][..4].copy_from_slice(&px);
        }
    };
    let be16 = |o: usize| u16::from_be_bytes([data[o], data[o + 1]]);

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let base = (ty * tiles_x + tx) * format.tile_bytes();
            let (x0, y0) = (tx * tw, ty * th);
            match format {
                TexFormat::S3tc => {
                    for sub in 0..4 {
                        let b = base + sub * 8;
                        let (sx, sy) = (x0 + (sub % 2) * 4, y0 + (sub / 2) * 4);
                        let (c0, c1) = (be16(b), be16(b + 2));
                        let (p0, p1) = (rgb565(c0), rgb565(c1));
                        let mix = |a: u8, b: u8, wa: u16, wb: u16| {
                            ((a as u16 * wa + b as u16 * wb) / (wa + wb)) as u8
                        };
                        let palette = if c0 > c1 {
                            [
                                p0,
                                p1,
                                std::array::from_fn(|i| if i == 3 { 255 } else { mix(p0[i], p1[i], 2, 1) }),
                                std::array::from_fn(|i| if i == 3 { 255 } else { mix(p0[i], p1[i], 1, 2) }),
                            ]
                        } else {
                            [
                                p0,
                                p1,
                                std::array::from_fn(|i| if i == 3 { 255 } else { mix(p0[i], p1[i], 1, 1) }),
                                [0, 0, 0, 0],
                            ]
                        };
                        for row in 0..4 {
                            let bits = data[b + 4 + row];
                            for col in 0..4 {
                                let idx = (bits >> (6 - col * 2)) & 3;
                                put(sx + col, sy + row, palette[idx as usize]);
                            }
                        }
                    }
                }
                TexFormat::Rgba8 => {
                    for i in 0..16 {
                        let (a, r) = (data[base + i * 2], data[base + i * 2 + 1]);
                        let (g, b) = (data[base + 32 + i * 2], data[base + 32 + i * 2 + 1]);
                        put(x0 + i % 4, y0 + i / 4, [r, g, b, a]);
                    }
                }
                _ => {
                    for i in 0..tw * th {
                        let (x, y) = (x0 + i % tw, y0 + i / tw);
                        let px = match format {
                            TexFormat::Rgb565 => rgb565(be16(base + i * 2)),
                            TexFormat::Rgb5a3 => rgb5a3(be16(base + i * 2)),
                            // GX samples intensity formats as I in all four channels (the
                            // decomp's PC path, `Texture::decodeData`, forces alpha to 255).
                            TexFormat::I4 => {
                                let byte = data[base + i / 2];
                                let v = if i % 2 == 0 { byte >> 4 } else { byte & 0xF } * 0x11;
                                [v, v, v, v]
                            }
                            TexFormat::I8 => {
                                let v = data[base + i];
                                [v, v, v, v]
                            }
                            TexFormat::Z8 => {
                                let v = data[base + i];
                                [v, v, v, 255]
                            }
                            TexFormat::Ia4 => {
                                let byte = data[base + i];
                                let (a, v) = ((byte >> 4) * 0x11, (byte & 0xF) * 0x11);
                                [v, v, v, a]
                            }
                            TexFormat::Ia8 => {
                                let (a, v) = (data[base + i * 2], data[base + i * 2 + 1]);
                                [v, v, v, a]
                            }
                            TexFormat::S3tc | TexFormat::Rgba8 => unreachable!(),
                        };
                        put(x, y, px);
                    }
                }
            }
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb5a3_opaque_and_translucent() {
        assert_eq!(rgb5a3(0xFFFF), [255, 255, 255, 255]);
        assert_eq!(rgb5a3(0x7F00), [255, 0, 0, 255]);
        assert_eq!(rgb5a3(0x0000), [0, 0, 0, 0]);
    }

    #[test]
    fn i8_tile_order() {
        // One 8x4 tile, pixels numbered row-major within the tile.
        let data: Vec<u8> = (0..32).collect();
        let img = decode(TexFormat::I8, 8, 4, &data).unwrap();
        let (x, y) = (3, 1);
        assert_eq!(img.rgba[(y * 8 + x) * 4], 11);
    }
}
