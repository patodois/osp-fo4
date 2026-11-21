//! Typed view of the Fallout 4 collision blocks.
//!
//! Layouts come from nifxml (`NiCollisionObject`, `bhkNPCollisionObject`,
//! `bhkSystem`, `bhkPhysicsSystem`, `bhkRagdollSystem`, `ByteArray`); see
//! `spec/collision.md`. The embedded binary data is returned as the exact
//! stored bytes with their file offset. Nothing here interprets it.
use crate::{Data, Nif, Value};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// The two concrete `bhkSystem` classes the schema declares for FO4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SystemKind {
    Physics,
    Ragdoll,
}
impl SystemKind {
    pub fn from_block_type(name: &str) -> Option<Self> {
        match name {
            "bhkPhysicsSystem" => Some(Self::Physics),
            "bhkRagdollSystem" => Some(Self::Ragdoll),
            _ => None,
        }
    }
    pub fn block_type(self) -> &'static str {
        match self {
            Self::Physics => "bhkPhysicsSystem",
            Self::Ragdoll => "bhkRagdollSystem",
        }
    }
}

/// One `bhkNPCollisionObject`, field for field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollisionObject {
    pub block: usize,
    /// File offset of the block's first byte.
    pub offset: u64,
    /// `Target` (Ptr to NiAVObject); -1 is null.
    pub target: i32,
    /// `Flags` (bhkCOFlags storage, u16), kept as stored.
    pub flags: u16,
    /// `Data` (Ref to bhkSystem); -1 is null.
    pub data: i32,
    /// `Body ID` (u32), kept as stored; its meaning is not established.
    pub body_id: u32,
}

/// One `bhkPhysicsSystem` or `bhkRagdollSystem`.
#[derive(Debug, Serialize)]
pub struct System<'a> {
    pub block: usize,
    pub kind: SystemKind,
    /// File offset of the u32 `Data Size`.
    pub size_offset: u64,
    /// File offset of the first byte of `Binary Data`.
    pub payload_offset: u64,
    /// The stored bytes, borrowed from the NIF buffer.
    #[serde(skip)]
    pub payload: &'a [u8],
    /// Bytes after the ByteArray inside the declared block size.
    pub tail: usize,
}

/// A block whose `Collision` field (NiAVObject) is not null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Owner {
    pub block: usize,
    pub field_offset: u64,
    pub collision: i32,
}

#[derive(Debug, Serialize)]
pub struct Collision<'a> {
    pub objects: Vec<CollisionObject>,
    pub systems: Vec<System<'a>>,
    pub owners: Vec<Owner>,
}
impl Collision<'_> {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.systems.is_empty() && self.owners.is_empty()
    }
    pub fn system(&self, block: i32) -> Option<&System<'_>> {
        self.systems
            .iter()
            .find(|s| block >= 0 && s.block == block as usize)
    }
}

fn decoded_fields<'n>(n: &'n Nif<'_>, index: usize) -> Result<Option<&'n [crate::Field]>> {
    let block = &n.blocks[index];
    if let Some(e) = block.diagnostics.first() {
        return Err(e.clone());
    }
    match &block.decoded {
        Some(Data::Fields(f)) => Ok(Some(f)),
        Some(Data::Node { fields, .. }) => Ok(Some(fields)),
        Some(Data::Mesh(m)) => Ok(Some(&m.fields)),
        None => Ok(None),
    }
}

fn field<'f>(fields: &'f [crate::Field], name: &str, file: &str, offset: u64) -> Result<&'f Value> {
    fields
        .iter()
        .find(|f| f.name == name)
        .map(|f| &f.value)
        .ok_or_else(|| Error::new("MissingNifCollisionField", file, offset, name, "absent"))
}

impl<'a> Nif<'a> {
    /// Collects the collision blocks and the NiAVObject links to them.
    ///
    /// Errors are named and carry the file offset: a collision block that did
    /// not decode (its diagnostic is returned), a collision block in a profile
    /// whose layout is not implemented, or a `Data` link whose target is not a
    /// `bhkSystem` subclass.
    pub fn collision(&self, file: &str, b: &Budget) -> Result<Collision<'a>> {
        let mut out = Collision {
            objects: Vec::new(),
            systems: Vec::new(),
            owners: Vec::new(),
        };
        for (index, block) in self.blocks.iter().enumerate() {
            let ty = self.block_types[block.type_index as usize].as_str();
            let offset = block.data.start as u64;
            let kind = SystemKind::from_block_type(ty);
            if ty == "bhkNPCollisionObject" || kind.is_some() {
                let Some(fields) = decoded_fields(self, index)? else {
                    return Err(Error::new(
                        "UnsupportedNifCollisionProfile",
                        file,
                        offset,
                        "Bethesda stream 130 collision layout",
                        format!("{ty} in BS version {}", self.bs_version),
                    ));
                };
                if let Some(kind) = kind {
                    let size_offset = fields
                        .iter()
                        .find(|f| f.name == "Data Size")
                        .map(|f| f.offset)
                        .ok_or_else(|| {
                            Error::new(
                                "MissingNifCollisionField",
                                file,
                                offset,
                                "Data Size",
                                "absent",
                            )
                        })?;
                    let Value::Raw(range) = field(fields, "Binary Data", file, offset)? else {
                        return Err(Error::new(
                            "InvalidNifCollisionField",
                            file,
                            offset,
                            "Binary Data as stored bytes",
                            "other value",
                        ));
                    };
                    b.reserve(&mut out.systems, 1, file, offset)?;
                    out.systems.push(System {
                        block: index,
                        kind,
                        size_offset,
                        payload_offset: range.start as u64,
                        payload: &self.bytes[range.clone()],
                        tail: block.raw_tail.len(),
                    });
                } else {
                    let (
                        Value::Ref(target),
                        Value::U16(flags),
                        Value::Ref(data),
                        Value::U32(body_id),
                    ) = (
                        field(fields, "Target", file, offset)?,
                        field(fields, "Flags", file, offset)?,
                        field(fields, "Data", file, offset)?,
                        field(fields, "Body ID", file, offset)?,
                    )
                    else {
                        return Err(Error::new(
                            "InvalidNifCollisionField",
                            file,
                            offset,
                            "Ptr, u16, Ref, u32",
                            "other values",
                        ));
                    };
                    b.reserve(&mut out.objects, 1, file, offset)?;
                    out.objects.push(CollisionObject {
                        block: index,
                        offset,
                        target: *target,
                        flags: *flags,
                        data: *data,
                        body_id: *body_id,
                    });
                }
            } else if let Ok(Some(fields)) = decoded_fields(self, index) {
                // A block that failed to decode reports its own diagnostic elsewhere.
                for f in fields.iter().filter(|f| f.name == "Collision") {
                    if let Value::Ref(collision) = f.value {
                        if collision >= 0 {
                            b.reserve(&mut out.owners, 1, file, f.offset)?;
                            out.owners.push(Owner {
                                block: index,
                                field_offset: f.offset,
                                collision,
                            });
                        }
                    }
                }
            }
        }
        for object in &out.objects {
            if object.data < 0 {
                continue;
            }
            let ty = &self.block_types[self.blocks[object.data as usize].type_index as usize];
            if SystemKind::from_block_type(ty).is_none() {
                return Err(Error::new(
                    "InvalidNifCollisionData",
                    file,
                    object.offset + 6,
                    "Ref to bhkSystem (bhkPhysicsSystem or bhkRagdollSystem)",
                    ty,
                ));
            }
        }
        Ok(out)
    }
}
