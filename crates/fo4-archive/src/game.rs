use crate::{Archive, BsaArchive, Texture};
use fo4_core::{Budget, Error, Game, Limits, ReadAt, Result, SourceSpan};
/// Format selection follows the explicit profile, never the magic or extension.
#[derive(Debug)]
pub enum GameArchive {
    Fallout4(Archive),
    SkyrimSe(BsaArchive),
}
pub struct EntryView<'a> {
    pub diagnostics: &'a [Error],
    pub name: &'a String,
    pub name_encoding: &'static str,
    pub original_name: &'a Vec<u8>,
    pub key: &'a Vec<u8>,
    pub texture: Option<&'a Texture>,
    pub source: &'a SourceSpan,
    pub chunks: usize,
}
impl GameArchive {
    pub fn read(game: Game, s: &dyn ReadAt, l: &Limits, b: &Budget) -> Result<Self> {
        match game {
            Game::Fallout4 => Archive::read(s, l, b).map(Self::Fallout4),
            Game::SkyrimSe => BsaArchive::read(s, l, b).map(Self::SkyrimSe),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Fallout4(a) => a.entries.len(),
            Self::SkyrimSe(a) => a.entries.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn version(&self) -> u32 {
        match self {
            Self::Fallout4(a) => a.version,
            Self::SkyrimSe(a) => a.version,
        }
    }
    pub fn kind(&self) -> [u8; 4] {
        match self {
            Self::Fallout4(a) => a.kind,
            Self::SkyrimSe(_) => *b"BSA\0",
        }
    }
    pub fn format(&self) -> &'static str {
        match self {
            Self::Fallout4(_) => "ba2",
            Self::SkyrimSe(_) => "bsa",
        }
    }
    pub fn uninterpreted_fields(&self) -> u64 {
        match self {
            Self::Fallout4(a) => a.uninterpreted_fields,
            Self::SkyrimSe(a) => a.uninterpreted_fields,
        }
    }
    pub fn entry(&self, index: usize) -> Result<EntryView<'_>> {
        let error = || Error::new("InvalidEntryIndex", "archive", 0, self.len(), index);
        Ok(match self {
            Self::Fallout4(a) => {
                let e = a.entries.get(index).ok_or_else(error)?;
                EntryView {
                    diagnostics: &e.diagnostics,
                    name: &e.name,
                    name_encoding: e.name_encoding,
                    original_name: &e.original_name,
                    key: &e.key,
                    texture: e.texture.as_ref(),
                    source: &e.source,
                    chunks: e.chunks.len(),
                }
            }
            Self::SkyrimSe(a) => {
                let e = a.entries.get(index).ok_or_else(error)?;
                EntryView {
                    diagnostics: &e.diagnostics,
                    name: &e.name,
                    name_encoding: e.name_encoding,
                    original_name: &e.original_name,
                    key: &e.key,
                    texture: None,
                    source: &e.source,
                    chunks: 1,
                }
            }
        })
    }
    pub fn codec(&self, index: usize, chunk: usize) -> Result<&'static str> {
        let e = self.entry(index)?;
        if chunk >= e.chunks {
            return Err(Error::new(
                "InvalidChunkIndex",
                &e.source.file,
                e.source.offset,
                e.chunks,
                chunk,
            ));
        }
        Ok(match self {
            Self::Fallout4(a) => {
                if a.entries[index].chunks[chunk].packed == 0 {
                    "raw"
                } else {
                    "zlib"
                }
            }
            Self::SkyrimSe(a) => {
                if a.entries[index].compressed {
                    "lz4-frame"
                } else {
                    "raw"
                }
            }
        })
    }
    pub fn decode_chunk(
        &self,
        s: &dyn ReadAt,
        index: usize,
        chunk: usize,
        l: &Limits,
        b: &Budget,
    ) -> Result<Vec<u8>> {
        self.codec(index, chunk)?;
        match self {
            Self::Fallout4(a) => a.decode_chunk(s, index, chunk, l, b),
            Self::SkyrimSe(a) => a.read_entry(s, index, l, b),
        }
    }
    pub fn read_entry(
        &self,
        s: &dyn ReadAt,
        index: usize,
        l: &Limits,
        b: &Budget,
    ) -> Result<Vec<u8>> {
        match self {
            Self::Fallout4(a) => a.read_entry(s, index, l, b),
            Self::SkyrimSe(a) => a.read_entry(s, index, l, b),
        }
    }
}
