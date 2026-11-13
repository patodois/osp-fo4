//! Optional lossless storage for our numerical maps; declared decoded size is mandatory.
use fo4_core::{Budget, Error, Limits, Result};
use std::io::Write;
struct Sink<'a> {
    bytes: Vec<u8>,
    budget: &'a Budget,
}
impl Write for Sink<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.budget
            .reserve(&mut self.bytes, data.len(), "map zlib", 0)
            .map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn pack(bytes: &[u8], b: &Budget) -> Result<Vec<u8>> {
    let scratch = b.child();
    scratch.charge(512 * 1024, "map zlib workspace", 0)?;
    let mut z = flate2::write::ZlibEncoder::new(
        Sink {
            bytes: Vec::new(),
            budget: b,
        },
        flate2::Compression::new(6),
    );
    z.write_all(bytes)
        .map_err(|e| Error::new("MapCompression", "map", 0, "zlib write", e))?;
    Ok(z.finish()
        .map_err(|e| Error::new("MapCompression", "map", 0, "finished zlib", e))?
        .bytes)
}
pub fn unpack(bytes: &[u8], declared: u64, l: &Limits, b: &Budget) -> Result<Vec<u8>> {
    fo4_core::zlib(bytes, declared, l, b, "numeric map", 0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_maps_reject_declared_sizes_and_trailing_input() {
        let bytes = [0u8, 0, 128, 127, 0, 0, 192, 127, 1, 2, 3, 4];
        let packed = pack(&bytes, &Budget::new(1 << 20)).unwrap();
        assert_eq!(
            unpack(&packed, 12, &Limits::default(), &Budget::new(1 << 20)).unwrap(),
            bytes
        );
        for size in [11, 13] {
            assert!(unpack(&packed, size, &Limits::default(), &Budget::new(1 << 20)).is_err());
        }
        let mut trailing = packed;
        trailing.push(0);
        assert!(unpack(&trailing, 12, &Limits::default(), &Budget::new(1 << 20)).is_err());
    }
}
