//! The three-byte form reference of the change-form table.
use serde::Serialize;

/// Three stored bytes read as a big-endian 24-bit word: the top two bits
/// select a [`RefKind`], the low 22 bits are the value
/// (`save_ref_id_kinds_v1`; the byte order is the one under which the
/// census resolves, see `spec/save.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct RefId(pub [u8; 3]);

/// Top two bits of a [`RefId`]. The labels are candidate meanings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum RefKind {
    /// 0: a 1-based index into the form id array.
    Indexed,
    /// 1: a form id of the first plugin, top byte 0.
    Direct,
    /// 2: a form id with top byte `0xFF`.
    Created,
    /// 3: not seen in the measured files.
    Other,
}

impl RefId {
    pub fn from_bytes(b: [u8; 3]) -> Self {
        Self(b)
    }
    /// The 24-bit word, first byte most significant.
    pub fn word(&self) -> u32 {
        u32::from(self.0[0]) << 16 | u32::from(self.0[1]) << 8 | u32::from(self.0[2])
    }
    pub fn kind(&self) -> RefKind {
        match self.0[0] >> 6 {
            0 => RefKind::Indexed,
            1 => RefKind::Direct,
            2 => RefKind::Created,
            _ => RefKind::Other,
        }
    }
    /// The low 22 bits.
    pub fn value(&self) -> u32 {
        self.word() & 0x3f_ffff
    }
}
