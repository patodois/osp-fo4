//! Collision layer records (`COLL`), read to the last byte
//! (`spec/record-layouts.tsv`, the `COLL` rows; `spec/record-layouts.md`,
//! section `coll`, which names the source). Layout as published for the
//! record type (UESP's file format page): `EDID` and `MNAM` texts, `DESC`
//! a localised string id, `BNAM` the layer's index, `FNAM` a debug colour,
//! `GNAM` a flag word, `INTV` the count of `CNAM`'s FormIDs, `CNAM` the
//! layer records this layer collides with. Nothing here says what a layer
//! does in the game: the rules that use the relation live in their callers
//! (`spec/hypotheses.tsv`).
use crate::{typed::Store, FormKey, Record};
use fo4_core::{Budget, Error, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The record type.
pub const COLL: [u8; 4] = *b"COLL";
/// Layer records read before [`read_all`] refuses the load order.
pub const MAX_LAYERS: usize = 4096;

/// One collision layer record, every subrecord kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layer {
    pub key: FormKey,
    /// `EDID` and `MNAM` without their closing zero.
    pub editor_id: Vec<u8>,
    pub name: Vec<u8>,
    /// `DESC`: a localised string id, not resolved.
    pub description: Option<u32>,
    /// `BNAM`: the layer's index.
    pub index: u32,
    /// `FNAM`: four bytes, as stored.
    pub color: [u8; 4],
    /// `GNAM`: the flag word, as stored.
    pub flags: u32,
    /// `INTV`: the count of `CNAM`'s FormIDs.
    pub count: u32,
    /// `CNAM`: the FormIDs as stored, and resolved through the load order.
    pub raw_collides: Vec<u32>,
    pub collides: Vec<FormKey>,
}

fn word(r: &Record, at: u64, d: &[u8], what: &str) -> Result<u32> {
    let Ok(d) = <[u8; 4]>::try_from(d) else {
        return Err(Error::new(
            "LayerFieldSize",
            &r.source.file,
            at,
            what,
            d.len(),
        ));
    };
    Ok(u32::from_le_bytes(d))
}

fn text(r: &Record, at: u64, d: &[u8]) -> Result<Vec<u8>> {
    match d.split_last() {
        Some((0, body)) if !body.contains(&0) => Ok(body.to_vec()),
        _ => Err(Error::new(
            "LayerText",
            &r.source.file,
            at,
            "text closed by one zero byte",
            d.len(),
        )),
    }
}

/// Reads the layer record `r` (key `key`) in the master context of
/// load-order plugin `plugin`. Every subrecord must be one the layout
/// names, once, with its size; `BNAM` must be present; `CNAM` must hold
/// `INTV` FormIDs. Otherwise a named error with file and offset.
pub fn read(r: &Record, key: FormKey, plugin: usize, store: &Store, b: &Budget) -> Result<Layer> {
    if r.signature != COLL {
        return Err(Error::new(
            "LayerRecordType",
            &r.source.file,
            r.source.offset,
            "COLL",
            String::from_utf8_lossy(&r.signature),
        ));
    }
    let mut seen: Vec<[u8; 4]> = Vec::new();
    let mut out = Layer {
        key,
        editor_id: Vec::new(),
        name: Vec::new(),
        description: None,
        index: 0,
        color: [0; 4],
        flags: 0,
        count: 0,
        raw_collides: Vec::new(),
        collides: Vec::new(),
    };
    let mut cnam: Option<(u64, &[u8])> = None;
    for s in &r.subrecords {
        let d = r.data(s);
        if seen.contains(&s.signature) {
            return Err(Error::new(
                "RepeatedLayerField",
                &r.source.file,
                s.offset,
                "each field once",
                String::from_utf8_lossy(&s.signature),
            ));
        }
        b.reserve(&mut seen, 1, &r.source.file, s.offset)?;
        seen.push(s.signature);
        match &s.signature {
            b"EDID" => out.editor_id = text(r, s.offset, d)?,
            b"MNAM" => out.name = text(r, s.offset, d)?,
            b"DESC" => out.description = Some(word(r, s.offset, d, "DESC: 4 bytes")?),
            b"BNAM" => out.index = word(r, s.offset, d, "BNAM: 4 bytes")?,
            b"FNAM" => {
                out.color = <[u8; 4]>::try_from(d).map_err(|_| {
                    Error::new(
                        "LayerFieldSize",
                        &r.source.file,
                        s.offset,
                        "FNAM: 4 bytes",
                        d.len(),
                    )
                })?
            }
            b"GNAM" => out.flags = word(r, s.offset, d, "GNAM: 4 bytes")?,
            b"INTV" => out.count = word(r, s.offset, d, "INTV: 4 bytes")?,
            b"CNAM" => cnam = Some((s.offset, d)),
            other => {
                return Err(Error::new(
                    "UnknownLayerField",
                    &r.source.file,
                    s.offset,
                    "EDID, DESC, BNAM, FNAM, GNAM, MNAM, INTV or CNAM",
                    String::from_utf8_lossy(other),
                ))
            }
        }
    }
    if !seen.contains(b"BNAM") {
        return Err(Error::new(
            "MissingLayerIndex",
            &r.source.file,
            r.source.offset,
            "BNAM",
            "absent",
        ));
    }
    let (at, d) = cnam.unwrap_or((r.source.offset, &[]));
    let n = d.len() / 4;
    if d.len() % 4 != 0 || n as u64 != u64::from(out.count) {
        return Err(Error::new(
            "LayerListCount",
            &r.source.file,
            at,
            format!("CNAM of INTV ({}) FormIDs", out.count),
            d.len(),
        ));
    }
    b.reserve(&mut out.raw_collides, n, &r.source.file, at)?;
    b.reserve(&mut out.collides, n, &r.source.file, at)?;
    for (k, c) in d.as_chunks::<4>().0.iter().enumerate() {
        let raw = u32::from_le_bytes(*c);
        let off = at + 4 * k as u64;
        let Some(f) = store.order().remap(plugin, raw, &r.source.file, off)? else {
            return Err(Error::new(
                "LayerListTarget",
                &r.source.file,
                off,
                "a FormID of the load order",
                format!("{raw:08x}"),
            ));
        };
        out.raw_collides.push(raw);
        out.collides.push(f);
    }
    Ok(out)
}

/// Every winning layer record of the load order, in the store's order.
pub fn read_all(store: &Store, b: &Budget) -> Result<Vec<Layer>> {
    let keys: Vec<FormKey> = store
        .winners()
        .filter(|o| o.signature == COLL && !o.deleted)
        .map(|o| store.overrides().form_key(o.key, store.order()))
        .collect();
    fo4_core::limit(keys.len() as u64, MAX_LAYERS as u64, "layer records", 0)?;
    let mut out = Vec::new();
    b.reserve(&mut out, keys.len(), "layer records", 0)?;
    for k in keys {
        let Some((r, plugin)) = store.raw_with_plugin(&k, b)? else {
            continue;
        };
        out.push(read(&r, k, plugin, store, b)?);
    }
    Ok(out)
}

/// The relation the layers' lists give, by index: for each layer's index,
/// the indexes of the layer records its `CNAM` names. Two records of one
/// index, or a `CNAM` naming a record that is not one of `layers`, is a
/// named error.
pub fn relation(layers: &[Layer]) -> Result<BTreeMap<u32, BTreeSet<u32>>> {
    let mut index: BTreeMap<&FormKey, u32> = BTreeMap::new();
    let mut out: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for l in layers {
        if out.insert(l.index, BTreeSet::new()).is_some() {
            return Err(Error::new(
                "RepeatedLayerIndex",
                &l.key.plugin,
                u64::from(l.key.object_id),
                "one layer record per index",
                l.index,
            ));
        }
        index.insert(&l.key, l.index);
    }
    for l in layers {
        for c in &l.collides {
            let Some(i) = index.get(c) else {
                return Err(Error::new(
                    "LayerListTarget",
                    &l.key.plugin,
                    u64::from(l.key.object_id),
                    "a layer record",
                    format!("{}:{:08x}", c.plugin, c.object_id),
                ));
            };
            if let Some(set) = out.get_mut(&l.index) {
                set.insert(*i);
            }
        }
    }
    Ok(out)
}

/// The pairs the relation lists one way only: (a, b) where a's list names
/// b and b's does not name a.
pub fn one_way(relation: &BTreeMap<u32, BTreeSet<u32>>) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for (a, set) in relation {
        for b in set {
            if !relation.get(b).is_some_and(|s| s.contains(a)) {
                out.push((*a, *b));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(index: u32, id: u32, collides: &[u32]) -> Layer {
        let k = |id: u32| FormKey {
            plugin: "a.esm".into(),
            object_id: id,
        };
        Layer {
            key: k(id),
            editor_id: Vec::new(),
            name: Vec::new(),
            description: None,
            index,
            color: [0; 4],
            flags: 0,
            count: collides.len() as u32,
            raw_collides: collides.to_vec(),
            collides: collides.iter().map(|c| k(*c)).collect(),
        }
    }

    #[test]
    fn the_relation_is_read_by_index_and_one_way_pairs_are_named() {
        let layers = [
            layer(0, 0x10, &[0x10, 0x11]),
            layer(5, 0x11, &[0x10]),
            layer(7, 0x12, &[0x11]),
        ];
        let r = relation(&layers).unwrap();
        assert_eq!(r[&0], BTreeSet::from([0, 5]));
        assert_eq!(r[&5], BTreeSet::from([0]));
        assert_eq!(r[&7], BTreeSet::from([5]));
        assert_eq!(one_way(&r), [(7, 5)]);
        // A list naming no layer record, and two records of one index.
        assert_eq!(
            relation(&[layer(0, 0x10, &[0x99])]).unwrap_err().code,
            "LayerListTarget"
        );
        assert_eq!(
            relation(&[layer(1, 0x10, &[]), layer(1, 0x11, &[])])
                .unwrap_err()
                .code,
            "RepeatedLayerIndex"
        );
    }
}
