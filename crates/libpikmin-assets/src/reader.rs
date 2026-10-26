//! Big-endian cursor matching `RandomAccessStream` (sysCommon/stream.cpp).
//!
//! The bounds-checked core is `libcommon::binary::Cursor`; the stream's own conventions
//! (vectors, counts, length-prefixed strings) are [`ReaderExt`].

use crate::Error;
use glam::{Vec2, Vec3};
use libcommon::binary::Truncated;

pub(crate) use libcommon::binary::Cursor as Reader;

impl From<Truncated> for Error {
    fn from(e: Truncated) -> Self {
        Error::Truncated(e.offset)
    }
}

pub(crate) trait ReaderExt {
    fn vec2(&mut self) -> Result<Vec2, Error>;
    fn vec3(&mut self) -> Result<Vec3, Error>;
    /// A count read as `i32`, rejecting negatives and anything bigger than the bytes left.
    fn count(&mut self) -> Result<usize, Error>;
    /// `Stream::readString`: `i32` length, then that many bytes.
    fn string(&mut self) -> Result<String, Error>;
}

impl ReaderExt for Reader<'_> {
    fn vec2(&mut self) -> Result<Vec2, Error> {
        Ok(Vec2::new(self.f32()?, self.f32()?))
    }

    fn vec3(&mut self) -> Result<Vec3, Error> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    fn count(&mut self) -> Result<usize, Error> {
        let at = self.pos();
        let n = self.i32()?;
        if n < 0 || n as usize > self.remaining() {
            return Err(Error::Bad(format!("count {n} at {at:#x}")));
        }
        Ok(n as usize)
    }

    fn string(&mut self) -> Result<String, Error> {
        let n = self.count()?;
        Ok(self.cstr_in(n)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_endian_reads_advance_and_failures_keep_position() {
        let d = [0x12, 0x34, 0x56, 0x78, 0xFF, 0xFE, 0x3F, 0x80, 0x00, 0x00, 0xAB];
        let mut r = Reader::new(&d);
        assert_eq!(r.u8().unwrap(), 0x12);
        assert_eq!(r.u16().unwrap(), 0x3456);
        r.seek(0);
        assert_eq!(r.u32().unwrap(), 0x1234_5678);
        assert_eq!(r.i16().unwrap(), -2);
        assert_eq!(r.f32().unwrap(), 1.0);
        assert_eq!((r.pos(), r.remaining()), (10, 1));
        assert!(matches!(r.u16().map_err(Error::from), Err(Error::Truncated(10))));
        assert_eq!(r.pos(), 10);
        r.seek(4);
        assert_eq!(r.i32().unwrap(), -0x0001_C080);
        assert_eq!(r.take(2).unwrap(), &[0x00, 0x00]);
        r.skip(1).unwrap();
        assert!(matches!(r.skip(1).map_err(Error::from), Err(Error::Truncated(11))));
        r.seek(100);
        assert_eq!(r.remaining(), 0);
        assert!(matches!(r.u8().map_err(Error::from), Err(Error::Truncated(100))));
    }

    #[test]
    fn align_vectors_counts_and_strings() {
        let mut r = Reader::new(&[0; 16]);
        r.skip(3).unwrap();
        r.align(4);
        assert_eq!(r.pos(), 4);
        r.align(4);
        assert_eq!(r.pos(), 4);

        let mut d = Vec::new();
        for f in [1.0f32, 2.0, 3.0] {
            d.extend_from_slice(&f.to_be_bytes());
        }
        let mut r = Reader::new(&d);
        assert_eq!(r.vec2().unwrap(), Vec2::new(1.0, 2.0));
        r.seek(0);
        assert_eq!(r.vec3().unwrap(), Vec3::new(1.0, 2.0, 3.0));

        let mut r = Reader::new(&[0, 0, 0, 3, 1, 2, 3]);
        assert_eq!(r.count().unwrap(), 3);
        for bad in [[0xFF, 0xFF, 0xFF, 0xFF], [0, 0, 0, 9]] {
            let mut r = Reader::new(&bad);
            assert!(matches!(r.count(), Err(Error::Bad(_))));
        }
        let mut r = Reader::new(&[0, 0, 0, 4, b'h', b'i', 0, b'x', 7]);
        assert_eq!(r.string().unwrap(), "hi");
        assert_eq!(r.u8().unwrap(), 7);
    }
}
