//! Frame validation from the LZ4 frame specification; only the block codec is delegated.
use fo4_core::{add, limit, Budget, Cursor, Error, Limits, Result, SliceSource};
fn hash(bytes: &[u8]) -> u32 {
    twox_hash::xxhash32::Hasher::oneshot(0, bytes)
}
pub(crate) fn decode(
    raw: &[u8],
    declared: u64,
    l: &Limits,
    b: &Budget,
    file: &str,
    offset: u64,
) -> Result<Vec<u8>> {
    let result = (|| {
        limit(declared, l.max_payload, file, 0)?;
        let source = SliceSource {
            name: file,
            bytes: raw,
        };
        let mut c = Cursor::new(&source);
        let magic = c.u32()?;
        if magic != 0x184d2204 {
            return Err(Error::new(
                "UnsupportedLz4Frame",
                file,
                0,
                "LZ4 frame magic 0x184D2204",
                magic,
            ));
        }
        let flg = c.u8()?;
        let bd = c.u8()?;
        if flg >> 6 != 1 || flg & 2 != 0 || bd & 0x8f != 0 {
            return Err(Error::new(
                "InvalidLz4Descriptor",
                file,
                4,
                "version 01, reserved bits zero",
                format!("{flg:#x}/{bd:#x}"),
            ));
        }
        let block_max = match (bd >> 4) & 7 {
            4 => 65536,
            5 => 262144,
            6 => 1048576,
            7 => 4194304,
            _ => {
                return Err(Error::new(
                    "UnsupportedLz4BlockSize",
                    file,
                    5,
                    "block maximum ID 4..7",
                    bd,
                ))
            }
        };
        if flg & 8 != 0 {
            let size = c.u64()?;
            if size != declared {
                return Err(Error::new(
                    "DecompressedSizeMismatch",
                    file,
                    6,
                    declared,
                    size,
                ));
            }
        }
        if flg & 1 != 0 {
            return Err(Error::new(
                "UnsupportedLz4Dictionary",
                file,
                c.pos,
                "no external dictionary",
                c.u32()?,
            ));
        }
        let end = c.pos as usize;
        let hc = c.u8()?;
        if hc != ((hash(&raw[4..end]) >> 8) & 255) as u8 {
            return Err(Error::new(
                "InvalidLz4HeaderChecksum",
                file,
                end as u64,
                "xxHash32 descriptor",
                hc,
            ));
        }
        let mut out = Vec::new();
        loop {
            let o = c.pos;
            let size = c.u32()?;
            if size == 0 {
                break;
            }
            let n = (size & 0x7fff_ffff) as u64;
            limit(n, block_max, file, o)?;
            let start = c.pos as usize;
            fo4_core::range(c.pos, n, c.end, file)?;
            c.pos = add(c.pos, n, file, c.pos)?;
            let stored = &raw[start..c.pos as usize];
            if flg & 16 != 0 {
                let check = c.u32()?;
                if check != hash(stored) {
                    return Err(Error::new(
                        "InvalidLz4BlockChecksum",
                        file,
                        c.pos - 4,
                        "xxHash32 stored block",
                        check,
                    ));
                }
            }
            let left = out.len();
            let capacity = add(declared, 1, file, o)?
                .saturating_sub(left as u64)
                .min(block_max) as usize;
            if size & 0x8000_0000 != 0 {
                if n as usize > capacity {
                    return Err(Error::new(
                        "DecompressedSizeMismatch",
                        file,
                        o,
                        declared,
                        add(left as u64, n, file, o)?,
                    ));
                }
                b.reserve_capped(
                    &mut out,
                    n as usize,
                    add(declared, 1, file, o)? as usize,
                    file,
                    o,
                )?;
                out.extend_from_slice(stored);
            } else {
                b.reserve_capped(
                    &mut out,
                    capacity,
                    add(declared, 1, file, o)? as usize,
                    file,
                    o,
                )?;
                out.resize(add(left as u64, capacity as u64, file, o)? as usize, 0);
                let (history, target) = out.split_at_mut(left);
                let dict = if flg & 32 != 0 {
                    &[][..]
                } else {
                    &history[history.len().saturating_sub(65536)..]
                };
                let count = lz4_flex::block::decompress_into_with_dict(stored, target, dict)
                    .map_err(|e| {
                        Error::new("InvalidLz4Block", file, o, "bounded valid block", e)
                    })?;
                out.truncate(add(left as u64, count as u64, file, o)? as usize);
            }
            if out.len() as u64 > declared {
                return Err(Error::new(
                    "DecompressedSizeMismatch",
                    file,
                    o,
                    declared,
                    out.len(),
                ));
            }
        }
        if flg & 4 != 0 {
            let check = c.u32()?;
            if check != hash(&out) {
                return Err(Error::new(
                    "InvalidLz4ContentChecksum",
                    file,
                    c.pos - 4,
                    "xxHash32 decoded content",
                    check,
                ));
            }
        }
        if c.pos != c.end {
            return Err(Error::new(
                "TrailingLz4Data",
                file,
                c.pos,
                "exactly one frame",
                c.end - c.pos,
            ));
        }
        if out.len() as u64 != declared {
            return Err(Error::new(
                "DecompressedSizeMismatch",
                file,
                c.pos,
                declared,
                out.len(),
            ));
        }
        Ok(out)
    })();
    result.map_err(
        |mut e: Error| match add(offset, e.source.offset, file, offset) {
            Ok(at) => {
                e.source.offset = at;
                e
            }
            Err(overflow) => overflow,
        },
    )
}
