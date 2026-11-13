use crate::{AlphaBlend, BlendMode, Path, WireBool};
use fo4_core::{add, limit, range, Cursor, Limits, Result};
pub(crate) struct Reader<'r, 's, 'a> {
    pub c: &'r mut Cursor<'s>,
    pub bytes: &'a [u8],
    pub version: u32,
    pub l: &'r Limits,
}
impl<'a> Reader<'_, '_, 'a> {
    pub fn byte(&mut self) -> Result<u8> {
        self.c.u8()
    }
    pub fn u32(&mut self) -> Result<u32> {
        self.c.u32()
    }
    pub fn boolean(&mut self) -> Result<WireBool> {
        Ok(WireBool(self.byte()?))
    }
    pub fn float(&mut self) -> Result<f32> {
        self.c.f32()
    }
    pub fn color(&mut self) -> Result<[f32; 3]> {
        Ok([self.float()?, self.float()?, self.float()?])
    }
    pub fn optional<T>(
        &mut self,
        present: bool,
        read: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<Option<T>> {
        if present {
            Ok(Some(read(self)?))
        } else {
            Ok(None)
        }
    }
    pub fn path(&mut self) -> Result<Path<'a>> {
        let at = self.c.pos;
        let len = self.u32()? as u64;
        limit(len, self.l.max_string, self.c.source.name(), at)?;
        range(self.c.pos, len, self.c.end, self.c.source.name())?;
        let start = self.c.pos;
        self.c.pos = add(start, len, self.c.source.name(), start)?;
        Ok(Path {
            offset: start,
            raw: &self.bytes[start as usize..self.c.pos as usize],
        })
    }
    pub fn blend(&mut self) -> Result<AlphaBlend> {
        let enabled = self.byte()?;
        let source = self.u32()?;
        let destination = self.u32()?;
        let mode = match (enabled, source, destination) {
            (0, 6, 7) => Some(BlendMode::Unknown),
            (0, 0, 0) => Some(BlendMode::None),
            (1, 6, 7) => Some(BlendMode::Standard),
            (1, 6, 0) => Some(BlendMode::Additive),
            (1, 4, 1) => Some(BlendMode::Multiplicative),
            _ => None,
        };
        Ok(AlphaBlend {
            enabled,
            source,
            destination,
            mode,
        })
    }
}
