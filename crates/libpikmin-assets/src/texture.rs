//! GameCube texture decoding to RGBA8.
//!
//! `TexImgFormat` (Texture.h) numbers the formats differently from GX; `Texture::decodeData`
//! (sysDolphin/texture.cpp) and `TexImg::getTileSize` (graphics.cpp) give the mapping and
//! tile sizes. The pixel layouts are the standard GX tiled formats.

use crate::Error;
use libcommon::texture::gx::{self, GxFormat};

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

    /// The shared GX decoder's format (`Texture::decodeData` maps these one to one).
    fn gx(self) -> GxFormat {
        match self {
            Self::Rgb565 => GxFormat::Rgb565,
            Self::S3tc => GxFormat::Cmpr,
            Self::Rgb5a3 => GxFormat::Rgb5a3,
            Self::I4 => GxFormat::I4,
            Self::I8 => GxFormat::I8,
            Self::Ia4 => GxFormat::Ia4,
            Self::Ia8 => GxFormat::Ia8,
            Self::Rgba8 => GxFormat::Rgba8,
            Self::Z8 => GxFormat::Z8,
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
    let rgba = gx::decode(format.gx(), width, height, data, &[])
        .map_err(|e| Error::Bad(e.to_string()))?;
    Ok(Image {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use libcommon::texture::gx::rgb5a3;

    #[test]
    fn rgb5a3_opaque_and_translucent() {
        assert_eq!(rgb5a3(0xFFFF), [255, 255, 255, 255]);
        assert_eq!(rgb5a3(0x7F00), [255, 0, 0, 255]);
        assert_eq!(rgb5a3(0x0000), [0, 0, 0, 0]);
    }

    fn fnv(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    }

    /// Pixel-exact baseline over pseudo-random data, including partial edge tiles, so a
    /// refactor of the decoder cannot change color or alpha unnoticed.
    #[test]
    fn decoded_pixels_match_baseline() {
        let mut s = 0x1234_5678u32;
        let data: Vec<u8> = (0..4096)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                (s >> 24) as u8
            })
            .collect();
        let got: Vec<u64> = (0..=8)
            .map(|f| {
                let format = TexFormat::from_tex_img(f).unwrap();
                fnv(&decode(format, 13, 11, &data).unwrap().rgba)
            })
            .collect();
        println!("{got:#x?}");
        assert_eq!(got, BASELINE);
    }

    // Recorded from the decoder before it moved to libcommon::texture::gx.
    const BASELINE: [u64; 9] = [
        0xa7e5dc0259b842b2,
        0xb9d7fb80fab30c30,
        0x433f0bf7c535a946,
        0x8951b18c7a118b4d,
        0x46ebc824be401321,
        0x9824912f42098bdf,
        0x7ba54e580471227b,
        0xb51c3975d39f7dfc,
        0xd4cc7b445e0f4bad,
    ];

    #[test]
    fn i8_tile_order() {
        // One 8x4 tile, pixels numbered row-major within the tile.
        let data: Vec<u8> = (0..32).collect();
        let img = decode(TexFormat::I8, 8, 4, &data).unwrap();
        let (x, y) = (3, 1);
        assert_eq!(img.rgba[(y * 8 + x) * 4], 11);
    }
}
