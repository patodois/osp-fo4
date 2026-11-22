//! A checked little-endian reader over one subrecord's bytes.
use fo4_core::{Error, Result};

pub(crate) struct Bytes<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    /// File name and file offset of `data[0]`, for errors.
    pub file: &'a str,
    pub base: u64,
}

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8], file: &'a str, base: u64) -> Self {
        Self {
            data,
            pos: 0,
            file,
            base,
        }
    }
    /// File offset of the current position.
    pub fn at(&self) -> u64 {
        self.base.saturating_add(self.pos as u64)
    }
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn error(&self, code: &str, expected: impl ToString, found: impl ToString) -> Error {
        Error::new(code, self.file, self.at(), expected, found)
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(self.error("NavTruncated", format!("{n} more bytes"), self.remaining()));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }
    pub fn vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    /// A count followed by `count * size` bytes that must fit in what is
    /// left; returns the count.
    pub fn count(&mut self, size: usize, what: &str) -> Result<u32> {
        let at = self.at();
        let n = self.u32()?;
        if n > crate::MAX_COUNT {
            return Err(crate::count_error(self.file, at, what, u64::from(n)));
        }
        let need = (n as usize).saturating_mul(size);
        if need > self.remaining() {
            return Err(Error::new(
                "NavTruncated",
                self.file,
                at,
                format!("{what}: {n} x {size} bytes"),
                format!("{} bytes left", self.remaining()),
            ));
        }
        Ok(n)
    }
    /// Fails unless every byte was read.
    pub fn finish(&self, what: &str) -> Result<()> {
        if self.remaining() != 0 {
            return Err(self.error(
                "NavTrailingBytes",
                format!("end of {what}"),
                format!("{} bytes left", self.remaining()),
            ));
        }
        Ok(())
    }
}
