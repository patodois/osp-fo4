//! Enable parents (XESP) of placed records, read to the last byte, and the
//! chain from a placed record up through its parents (`spec/plugin.md`,
//! Enable parent). Layout: a FormID, one flag byte and three bytes, named
//! by the published record definitions cited in `spec/plugin.md`; the
//! flag bits and the presence a chain gives are not read here: the rules
//! that decide them live in `fo4_scene::enable` (`spec/hypotheses.tsv`).
use crate::{typed::Store, FormKey, Record, DISABLED};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;

/// Enable parent subrecord signature.
pub const XESP: [u8; 4] = *b"XESP";
/// Longest chain followed before [`ChainEnd::TooDeep`].
pub const MAX_DEPTH: usize = 64;

/// One XESP, every byte kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnableLink {
    /// Stored FormID, in the file's master context.
    pub raw: u32,
    /// `raw` resolved through the load order; `None` for 0 or an index
    /// past the masters.
    pub parent: Option<FormKey>,
    /// Byte 4.
    pub flags: u8,
    /// Bytes 5 to 7, not interpreted.
    pub reserved: [u8; 3],
    /// Index of the subrecord in the record.
    pub subrecord: usize,
}

/// The XESP of `r` read in the master context of load-order plugin
/// `plugin`; `None` without one. More than one XESP, or one that is not 8
/// bytes, is a named error.
pub fn read(r: &Record, plugin: usize, store: &Store) -> Result<Option<EnableLink>> {
    let mut found = None;
    for (i, s) in r.subrecords.iter().enumerate() {
        if s.signature != XESP {
            continue;
        }
        if found.is_some() {
            return Err(Error::new(
                "RepeatedEnableParent",
                &r.source.file,
                s.offset,
                "one XESP",
                "two or more",
            ));
        }
        let d = r.data(s);
        let Ok(d) = <[u8; 8]>::try_from(d) else {
            return Err(Error::new(
                "EnableParentSize",
                &r.source.file,
                s.offset,
                "8 bytes",
                d.len(),
            ));
        };
        let raw = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        let parent = if raw == 0 {
            None
        } else {
            store.order().remap(plugin, raw, &r.source.file, s.offset)?
        };
        found = Some(EnableLink {
            raw,
            parent,
            flags: d[4],
            reserved: [d[5], d[6], d[7]],
            subrecord: i,
        });
    }
    Ok(found)
}

/// One record of a chain: its key, its record type, whether its header
/// carries the initially-disabled flag, and its own XESP.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub key: FormKey,
    pub signature: [u8; 4],
    pub initially_disabled: bool,
    pub deleted: bool,
    pub link: Option<EnableLink>,
}

/// How a chain ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ChainEnd {
    /// The last node has no XESP.
    Root,
    /// The last node's XESP names FormID 0.
    NullParent,
    /// The last node's XESP names a FormID the load order cannot resolve.
    UnresolvedParent { raw: u32 },
    /// The last node's XESP names a key with no winning record.
    MissingParent(FormKey),
    /// The last node's XESP names a node already in the chain.
    Cycle(FormKey),
    /// More than [`MAX_DEPTH`] links.
    TooDeep,
}

/// A placed record followed up through its enable parents: `nodes[0]` is
/// the record, `nodes[k + 1]` the parent `nodes[k]`'s XESP names.
#[derive(Debug, Clone, Serialize)]
pub struct Chain {
    pub nodes: Vec<Node>,
    pub end: ChainEnd,
}
impl Chain {
    /// Number of XESP links followed to the end.
    pub fn depth(&self) -> usize {
        self.nodes.len().saturating_sub(1)
    }
}

/// Reads the winning record `key` and its parents. `Ok(None)` when `key`
/// has no winning record.
pub fn chain(store: &Store, key: &FormKey, b: &Budget) -> Result<Option<Chain>> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut next = key.clone();
    loop {
        let work = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&next, &work)? else {
            if nodes.is_empty() {
                return Ok(None);
            }
            return Ok(Some(Chain {
                nodes,
                end: ChainEnd::MissingParent(next),
            }));
        };
        let link = read(&r, plugin, store)?;
        b.reserve(&mut nodes, 1, &r.source.file, r.source.offset)?;
        b.charge(next.plugin.len() as u64, &r.source.file, r.source.offset)?;
        nodes.push(Node {
            key: next.clone(),
            signature: r.signature,
            initially_disabled: r.flags & DISABLED != 0,
            deleted: r.deleted(),
            link: link.clone(),
        });
        let end = match link {
            None => Some(ChainEnd::Root),
            Some(l) if l.raw == 0 => Some(ChainEnd::NullParent),
            Some(EnableLink {
                parent: None, raw, ..
            }) => Some(ChainEnd::UnresolvedParent { raw }),
            Some(EnableLink {
                parent: Some(p), ..
            }) => {
                if nodes.iter().any(|n| n.key == p) {
                    Some(ChainEnd::Cycle(p))
                } else if nodes.len() > MAX_DEPTH {
                    Some(ChainEnd::TooDeep)
                } else {
                    next = p;
                    None
                }
            }
        };
        if let Some(end) = end {
            return Ok(Some(Chain { nodes, end }));
        }
    }
}
