//! `.stx` streams (cutscene and jingle audio): `StreamHeader_` (syncstream.h) then stereo AFC.
//!
//! `StreamAudio_Start` reads the 0x20-byte header and streams the rest in 0x2400-byte chunks.
//! Format 4 (`AUDIOFRMT_ADPCM`, `__DecodeADPCM`) stores each 16 stereo samples as a 9-byte
//! left frame then a 9-byte right frame. Format 5 (`AUDIOFRMT_ADPCM4X`, `__DecodeADPCM4X`) holds
//! two stereo pairs per 36 bytes that are mixed at `mixLevel` (0x5FFF each by default).
//! Decoder state resets only at the start of the stream.

use super::adpcm::{History, decode_frame4};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamHeader {
    /// Bytes after the header.
    pub data_size: u32,
    pub samples: u32,
    pub sample_rate: u16,
    pub format: u16,
    pub frame_rate: u16,
}

/// A decoded stream: interleaved stereo.
#[derive(Debug, Clone)]
pub struct Stream {
    pub header: StreamHeader,
    pub stereo: Vec<i16>,
}

impl StreamHeader {
    pub fn parse(d: &[u8]) -> Result<Self, Error> {
        if d.len() < 0x20 {
            return Err(Error::Truncated(d.len()));
        }
        let u32_at = |o: usize| u32::from_be_bytes(d[o..o + 4].try_into().unwrap());
        let u16_at = |o: usize| u16::from_be_bytes(d[o..o + 2].try_into().unwrap());
        Ok(StreamHeader {
            data_size: u32_at(0),
            samples: u32_at(4),
            sample_rate: u16_at(8),
            format: u16_at(0xA),
            frame_rate: u16_at(0xE),
        })
    }
}

impl Stream {
    pub fn decode(d: &[u8]) -> Result<Self, Error> {
        let header = StreamHeader::parse(d)?;
        let end = (0x20 + header.data_size as usize).min(d.len());
        let data = &d[0x20..end];
        let n = header.samples as usize;
        let mut stereo = Vec::with_capacity(n * 2);
        let (mut l, mut r) = ([0i16; 16], [0i16; 16]);
        match header.format {
            4 => {
                let (mut hl, mut hr) = (History::default(), History::default());
                for f in data.as_chunks::<18>().0 {
                    decode_frame4(&f[..9], &mut hl, &mut l);
                    decode_frame4(&f[9..], &mut hr, &mut r);
                    for i in 0..16 {
                        stereo.extend_from_slice(&[l[i], r[i]]);
                    }
                }
            }
            5 => {
                const MIX: i32 = 0x5FFF;
                let mut h = [History::default(); 4];
                let (mut l2, mut r2) = ([0i16; 16], [0i16; 16]);
                for f in data.as_chunks::<36>().0 {
                    decode_frame4(&f[..9], &mut h[0], &mut l);
                    decode_frame4(&f[9..18], &mut h[1], &mut r);
                    decode_frame4(&f[18..27], &mut h[2], &mut l2);
                    decode_frame4(&f[27..], &mut h[3], &mut r2);
                    let mix = |a: i16, b: i16| {
                        (((a as i32 * MIX) >> 15) + ((b as i32 * MIX) >> 15)).clamp(-0x8000, 0x7FFF)
                            as i16
                    };
                    for i in 0..16 {
                        stereo.extend_from_slice(&[mix(l[i], l2[i]), mix(r[i], r2[i])]);
                    }
                }
            }
            f => return Err(Error::Bad(format!("stream format {f} not supported"))),
        }
        // The last frame can be cut short; the header's count wins.
        stereo.resize(n * 2, 0);
        Ok(Stream { header, stereo })
    }
}
