//! Big-endian cursor matching `RandomAccessStream` (sysCommon/stream.cpp).

use crate::Error;
use glam::{Vec2, Vec3};

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let s = self
            .data
            .get(self.pos..self.pos + n)
            .ok_or(Error::Truncated(self.pos))?;
        self.pos += n;
        Ok(s)
    }

    pub fn skip(&mut self, n: usize) -> Result<(), Error> {
        self.take(n).map(|_| ())
    }

    /// `RandomAccessStream::skipPadding`: advance to the next multiple of `align`.
    pub fn align(&mut self, align: usize) {
        self.pos = self.pos.next_multiple_of(align);
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn i16(&mut self) -> Result<i16, Error> {
        Ok(self.u16()? as i16)
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }

    pub fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn vec2(&mut self) -> Result<Vec2, Error> {
        Ok(Vec2::new(self.f32()?, self.f32()?))
    }

    pub fn vec3(&mut self) -> Result<Vec3, Error> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    /// A count read as `i32`, rejecting negatives and anything bigger than the bytes left.
    pub fn count(&mut self) -> Result<usize, Error> {
        let at = self.pos;
        let n = self.i32()?;
        if n < 0 || n as usize > self.remaining() {
            return Err(Error::Bad(format!("count {n} at {at:#x}")));
        }
        Ok(n as usize)
    }

    /// `Stream::readString`: `i32` length, then that many bytes.
    pub fn string(&mut self) -> Result<String, Error> {
        let n = self.count()?;
        let raw = self.take(n)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
    }
}
