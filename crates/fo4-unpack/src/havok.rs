//! Havok container census: counts and small stored values only.
//!
//! Reads every bhkSystem payload of the winning NIFs with
//! `fo4_havok::Container` and counts, per system kind, how often each
//! container-level property holds. Properties the reader enforces are counted
//! by the reads that succeed; the others are counted here, holds and fails.
//! Nothing here decodes an object.
use crate::Reports;
use fo4_core::{Budget, Manifest, Result};
use fo4_havok::{Container, Object};
use fo4_nif::Nif;
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const CAP: usize = 256;
const OTHER: &str = "<other>";
/// 16-byte groups after the first 16 bytes of the root object whose third
/// u32 is compared with the number of collision objects on the system.
const ROOT_GROUPS: usize = 8;

fn bump<K: Ord + Clone>(map: &mut BTreeMap<K, u64>, key: K, other: K) {
    let key = if map.contains_key(&key) || map.len() < CAP {
        key
    } else {
        other
    };
    *map.entry(key).or_default() += 1;
}

#[derive(Default, Serialize, Clone, Copy)]
pub struct Check {
    pub holds: u64,
    pub fails: u64,
}

#[derive(Default, Serialize)]
pub struct Class {
    /// Payloads whose `__classnames__` lists the name.
    pub listed: u64,
    /// Payloads with at least one object of the class.
    pub payloads: u64,
    pub objects: u64,
    pub signatures: BTreeMap<u32, u64>,
    /// Payloads where the number of objects of the class equals the number of
    /// bhkNPCollisionObject on the system, and where it differs.
    pub objects_eq_system_objects: Check,
}

#[derive(Default, Serialize)]
pub struct Kind {
    pub payloads: u64,
    pub read: u64,
    pub errors: BTreeMap<String, u64>,
    pub checks: BTreeMap<&'static str, Check>,
    pub classes: BTreeMap<String, Class>,
    pub markers: BTreeMap<u8, u64>,
    /// "table|bytes" -> payloads.
    pub fill: BTreeMap<String, u64>,
    pub totals: BTreeMap<&'static str, u64>,
    pub maxima: BTreeMap<&'static str, u64>,
    pub header_words: BTreeMap<String, u64>,
    pub versions: BTreeMap<String, u64>,
    /// Classes of objects whose offset is also a first-table source.
    pub pointer_source_is_object: BTreeMap<String, u64>,
    /// Classes of the objects second-table entries point to.
    pub object_pointer_targets: BTreeMap<String, u64>,
    pub root_class: BTreeMap<String, u64>,
    /// "root class|+offset" -> u32 there equal / not equal to the system's objects.
    pub root_counts: BTreeMap<String, Check>,
    /// "root class|+group offset|property" -> holds / fails.
    pub root_groups: BTreeMap<String, Check>,
}
impl Kind {
    fn check(&mut self, name: &'static str, ok: bool) {
        let c = self.checks.entry(name).or_default();
        if ok {
            c.holds += 1;
        } else {
            c.fails += 1;
        }
    }
    fn total(&mut self, name: &'static str, n: u64) {
        *self.totals.entry(name).or_default() += n;
        let m = self.maxima.entry(name).or_default();
        *m = (*m).max(n);
    }
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub nif_files: u64,
    pub nif_read: u64,
    pub nif_failed: u64,
    pub collision_view_failed: u64,
    pub kinds: BTreeMap<String, Kind>,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let w = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
}

/// Measures one payload; `objects` is the number of bhkNPCollisionObject on its system.
fn payload(
    k: &mut Kind,
    bytes: &[u8],
    objects: u64,
    file: &str,
    base: u64,
    b: &Budget,
) -> Result<()> {
    k.payloads += 1;
    // Byte order: record starts read big-endian against the payload length.
    let be_in_bounds = (0..3).all(|i| {
        bytes
            .get(84 + 64 * i..88 + 64 * i)
            .map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]) as usize)
            .is_some_and(|v| v <= bytes.len())
    });
    k.check("record_starts_read_big_endian_in_bounds", be_in_bounds);
    let c = match Container::read(bytes, file, base, b) {
        Ok(c) => c,
        Err(e) => {
            bump(&mut k.errors, e.code.clone(), OTHER.into());
            return Err(e);
        }
    };
    k.read += 1;
    for (i, w) in c.header.words.iter().enumerate() {
        bump(
            &mut k.header_words,
            format!("{:02}|{w:08x}", i * 4),
            OTHER.into(),
        );
    }
    bump(&mut k.versions, c.version.to_owned(), OTHER.into());
    // Always present after a successful read.
    let Some(data) = c.data() else {
        return Ok(());
    };
    let d = data.start as usize;
    k.check(
        "section_starts_and_offsets_multiple_of_16",
        c.sections
            .iter()
            .all(|s| s.start.is_multiple_of(16) && s.offsets.iter().all(|o| o.is_multiple_of(16))),
    );
    k.check(
        "section_records_byte19_and_48_to_63_are_ff",
        c.sections.iter().all(|s| {
            let r = s.record as usize;
            bytes.get(r + 19) == Some(&0xff)
                && bytes
                    .get(r + 48..r + 64)
                    .is_some_and(|x| x.iter().all(|v| *v == 0xff))
        }),
    );
    k.check(
        "section_record_name_padding_zero",
        c.sections.iter().all(|s| {
            let r = s.record as usize;
            bytes
                .get(r + s.name.len()..r + 19)
                .is_some_and(|x| x.iter().all(|v| *v == 0))
        }),
    );
    k.total("class_names", c.class_names.len() as u64);
    k.total("pointers", c.pointers.len() as u64);
    k.total("object_pointers", c.object_pointers.len() as u64);
    k.total("objects", c.objects.len() as u64);
    k.total("object_bytes", data.offsets[0] as u64);
    for (table, fill) in ["classnames", "pointers", "object_pointers", "objects"]
        .iter()
        .zip(c.fill)
    {
        bump(&mut k.fill, format!("{table}|{fill:02}"), OTHER.into());
    }
    k.check("classnames_fill_below_16", c.fill[0] < 16);
    k.check("pointer_fill_below_16", c.fill[1] < 16);
    k.check("object_pointer_fill_below_16", c.fill[2] < 16);
    k.check("object_fill_below_16", c.fill[3] < 16);
    let mut listed = BTreeSet::new();
    for n in &c.class_names {
        bump(&mut k.markers, n.marker, 0);
        listed.insert(n.name);
        let class = k.classes.entry(n.name.to_owned()).or_default();
        class.listed += 1;
        bump(&mut class.signatures, n.signature, u32::MAX);
    }
    k.check("class_names_distinct", listed.len() == c.class_names.len());
    let mut per_class: BTreeMap<&str, u64> = BTreeMap::new();
    for o in &c.objects {
        *per_class.entry(o.class).or_default() += 1;
    }
    for name in &listed {
        let n = per_class.get(name).copied().unwrap_or(0);
        let class = k.classes.entry((*name).to_owned()).or_default();
        if n > 0 {
            class.payloads += 1;
            class.objects += n;
        }
        let check = &mut class.objects_eq_system_objects;
        if n == objects {
            check.holds += 1;
        } else {
            check.fails += 1;
        }
    }
    let zero = |source: u32| {
        bytes
            .get(d + source as usize..d + source as usize + 8)
            .is_some_and(|x| x.iter().all(|v| *v == 0))
    };
    k.check(
        "pointer_source_multiple_of_8",
        c.pointers.iter().all(|p| p.source.is_multiple_of(8)),
    );
    k.check(
        "pointer_destination_multiple_of_16",
        c.pointers.iter().all(|p| p.destination.is_multiple_of(16)),
    );
    k.check(
        "object_pointer_source_multiple_of_8",
        c.object_pointers.iter().all(|p| p.source.is_multiple_of(8)),
    );
    k.check(
        "object_offset_multiple_of_16",
        c.objects.iter().all(|o| o.offset.is_multiple_of(16)),
    );
    k.check(
        "pointer_sources_increasing",
        c.pointers.windows(2).all(|w| w[0].source < w[1].source),
    );
    k.check(
        "pointer_sources_increasing_iff_no_hknpCompressedMeshShape_listed",
        c.pointers.windows(2).all(|w| w[0].source < w[1].source)
            != c.class_names
                .iter()
                .any(|n| n.name == "hknpCompressedMeshShape"),
    );
    k.check(
        "object_pointer_sources_increasing",
        c.object_pointers
            .windows(2)
            .all(|w| w[0].source < w[1].source),
    );
    let mut pointer_sources: Vec<u32> = c.pointers.iter().map(|p| p.source).collect();
    pointer_sources.sort_unstable();
    let mut link_sources: Vec<u32> = c.object_pointers.iter().map(|p| p.source).collect();
    link_sources.sort_unstable();
    k.check(
        "pointer_sources_distinct",
        pointer_sources.windows(2).all(|w| w[0] != w[1]),
    );
    k.check(
        "object_pointer_sources_distinct",
        link_sources.windows(2).all(|w| w[0] != w[1]),
    );
    k.check(
        "pointer_and_object_pointer_sources_disjoint",
        link_sources
            .iter()
            .all(|s| pointer_sources.binary_search(s).is_err()),
    );
    let mut all: Vec<u32> = pointer_sources
        .iter()
        .chain(&link_sources)
        .copied()
        .collect();
    all.sort_unstable();
    k.check(
        "pointer_and_object_pointer_sources_8_apart",
        all.windows(2).all(|w| w[1] - w[0] >= 8),
    );
    k.check(
        "eight_zero_bytes_at_every_pointer_source",
        c.pointers.iter().all(|p| zero(p.source)),
    );
    k.check(
        "eight_zero_bytes_at_every_object_pointer_source",
        c.object_pointers.iter().all(|p| zero(p.source)),
    );
    k.check(
        "eight_zero_bytes_at_every_object_offset",
        c.objects.iter().all(|o| zero(o.offset)),
    );
    let find = |offset: u32| -> Option<&Object<'_>> {
        c.objects
            .binary_search_by_key(&offset, |o| o.offset)
            .ok()
            .map(|i| &c.objects[i])
    };
    k.check(
        "pointer_destination_not_an_object_offset",
        c.pointers.iter().all(|p| find(p.destination).is_none()),
    );
    for p in &c.pointers {
        if let Some(o) = find(p.source) {
            bump(
                &mut k.pointer_source_is_object,
                o.class.to_owned(),
                OTHER.into(),
            );
        }
    }
    k.check(
        "object_pointer_source_not_an_object_offset",
        c.object_pointers.iter().all(|p| find(p.source).is_none()),
    );
    for p in &c.object_pointers {
        if let Some(o) = find(p.destination) {
            bump(
                &mut k.object_pointer_targets,
                o.class.to_owned(),
                OTHER.into(),
            );
        }
    }
    k.check(
        "root_is_first_object",
        c.objects.first().map(|o| o.offset) == Some(c.header.root_offset()),
    );
    k.check(
        "root_class_is_fifth_class_name",
        c.class_names.get(4).map(|n| n.offset) == Some(c.header.root_class_offset()),
    );
    k.check(
        "first_four_class_names_never_objects",
        c.class_names
            .iter()
            .take(4)
            .all(|n| !per_class.contains_key(n.name)),
    );
    if let Some(root) = c.root() {
        bump(&mut k.root_class, root.class.to_owned(), OTHER.into());
        let at = d + root.offset as usize;
        for g in 0..ROOT_GROUPS {
            let off = 16 + 16 * g + 8;
            let key = format!("{}|+{off:03}", root.class);
            let check = k.root_counts.entry(key).or_default();
            let count = u32_at(bytes, at + off);
            if count.map(u64::from) == Some(objects) {
                check.holds += 1;
            } else {
                check.fails += 1;
            }
            // The u32 after the compared one, and whether the group's first
            // eight bytes are a first-table source.
            let next = u32_at(bytes, at + off + 4);
            let start = root.offset + 16 + 16 * g as u32;
            let pointer = c.pointers.iter().any(|p| p.source == start);
            for (name, ok) in [
                (
                    "next_u32_is_0x80000000_or_count",
                    count.zip(next).is_some_and(|(n, x)| x == 0x8000_0000 | n),
                ),
                (
                    "pointer_source_at_group_start_iff_count_nonzero",
                    count.is_some_and(|n| pointer == (n > 0)),
                ),
            ] {
                let check = k
                    .root_groups
                    .entry(format!("{}|+{:03}|{name}", root.class, off - 8))
                    .or_default();
                if ok {
                    check.holds += 1;
                } else {
                    check.fails += 1;
                }
            }
        }
    }
    Ok(())
}

pub fn sweep(m: &Manifest, r: &mut Reports) -> Result<()> {
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.vfs_memory = v.memory_used();
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets".into());
    let mut s = Summary::default();
    for chain in v.candidates.chunk_by(|a, b| a.key == b.key) {
        let Some(c) = chain.last() else { continue };
        let Some(key) = &c.key else { continue };
        if !key.ends_with(b".nif") {
            continue;
        }
        s.nif_files += 1;
        let work = Budget::new(m.limits.max_memory);
        let bytes = match v.read_candidate(c, &work) {
            Ok(bytes) => bytes,
            Err(e) => {
                s.nif_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        let n = match Nif::read(&bytes, &c.original, m.game, &m.limits, &work) {
            Ok(n) => n,
            Err(e) => {
                s.nif_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        s.nif_read += 1;
        let col = match n.collision(&c.original, &work) {
            Ok(col) => col,
            Err(e) => {
                s.collision_view_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        for system in &col.systems {
            let objects = col
                .objects
                .iter()
                .filter(|o| o.data == system.block as i32)
                .count() as u64;
            let kind = s
                .kinds
                .entry(system.kind.block_type().to_owned())
                .or_default();
            let scratch = Budget::new(m.limits.max_memory);
            if let Err(e) = payload(
                kind,
                system.payload,
                objects,
                &c.original,
                system.payload_offset,
                &scratch,
            ) {
                r.error(&e)?;
            }
        }
    }
    let mut text = String::new();
    for (k, value) in [
        (
            "build",
            m.build.clone().unwrap_or_else(|| "undeclared".into()),
        ),
        ("plugins", m.plugins.len().to_string()),
        ("mounts", m.mounts.len().to_string()),
        (
            "vfs_fingerprint",
            r.summary.fingerprint.clone().unwrap_or_default(),
        ),
    ] {
        text.push_str(&format!("census\t{k}\t{value}\n"));
    }
    text.push_str(&tsv(&s));
    r.tsv = Some(("havok.tsv", text));
    r.summary.havok = s;
    Ok(())
}

/// Text report: `section<TAB>key<TAB>value`, counts and small stored values.
pub fn tsv(s: &Summary) -> String {
    let mut out = String::from("# section\tkey\tvalue\n");
    let mut row = |section: &str, key: &str, value: String| {
        out.push_str(section);
        out.push('\t');
        out.push_str(key);
        out.push('\t');
        out.push_str(&value);
        out.push('\n');
    };
    for (k, v) in [
        ("nif_files", s.nif_files),
        ("nif_read", s.nif_read),
        ("nif_failed", s.nif_failed),
        ("collision_view_failed", s.collision_view_failed),
    ] {
        row("total", k, v.to_string());
    }
    let check = |c: &Check| format!("holds={} fails={}", c.holds, c.fails);
    for (kind, k) in &s.kinds {
        let sec = |name: &str| format!("havok.{kind}.{name}");
        row(&sec("read"), "payloads", k.payloads.to_string());
        row(&sec("read"), "read", k.read.to_string());
        for (code, n) in &k.errors {
            row(&sec("error"), code, n.to_string());
        }
        for (name, c) in &k.checks {
            row(&sec("check"), name, check(c));
        }
        for (name, n) in &k.totals {
            row(&sec("total"), name, n.to_string());
        }
        for (name, n) in &k.maxima {
            row(&sec("max_per_payload"), name, n.to_string());
        }
        for (key, n) in &k.header_words {
            row(&sec("header_word"), key, n.to_string());
        }
        for (key, n) in &k.versions {
            row(&sec("version"), key, n.to_string());
        }
        for (key, n) in &k.markers {
            row(&sec("class_name_marker"), &key.to_string(), n.to_string());
        }
        for (key, n) in &k.fill {
            row(&sec("fill"), key, n.to_string());
        }
        for (name, c) in &k.classes {
            let signatures: Vec<String> = c
                .signatures
                .iter()
                .map(|(sig, n)| format!("{sig:08x}x{n}"))
                .collect();
            row(
                &sec("class"),
                name,
                format!(
                    "listed={} payloads={} objects={} signatures={} count_eq_system_objects={}",
                    c.listed,
                    c.payloads,
                    c.objects,
                    signatures.join(","),
                    check(&c.objects_eq_system_objects)
                ),
            );
        }
        for (key, n) in &k.pointer_source_is_object {
            row(&sec("pointer_source_is_object"), key, n.to_string());
        }
        for (key, n) in &k.object_pointer_targets {
            row(&sec("object_pointer_target"), key, n.to_string());
        }
        for (key, n) in &k.root_class {
            row(&sec("root_class"), key, n.to_string());
        }
        for (key, c) in &k.root_counts {
            row(&sec("root_u32_eq_system_objects"), key, check(c));
        }
        for (key, c) in &k.root_groups {
            row(&sec("root_group"), key, check(c));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic container: root object with one group whose third u32 is 2.
    fn container() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(fo4_havok::MAGIC);
        out.extend(0u32.to_le_bytes());
        out.extend(11u32.to_le_bytes());
        out.extend(fo4_havok::LAYOUT);
        for w in [3u32, 2, 0, 0, 5] {
            out.extend(w.to_le_bytes());
        }
        out.extend(b"hk_2014.1.0-r1\0\xff");
        out.extend([0u8; 8]);
        let mut names = Vec::new();
        names.extend(7u32.to_le_bytes());
        names.push(9);
        names.extend(b"Root\0");
        names.resize(16, 0xff);
        let mut data = vec![0u8; 0x40];
        data[0x18..0x1c].copy_from_slice(&2u32.to_le_bytes());
        data[0x1c..0x20].copy_from_slice(&0x8000_0002u32.to_le_bytes());
        for w in [0x10u32, 0x30] {
            data.extend(w.to_le_bytes());
        }
        data.resize(0x50, 0xff);
        data.resize(0x60, 0xff);
        for w in [0u32, 0, 5] {
            data.extend(w.to_le_bytes());
        }
        data.resize(0x70, 0xff);
        for (i, (name, start, words)) in [
            ("__classnames__", 256u32, [16u32; 6]),
            ("__types__", 272, [0; 6]),
            ("__data__", 272, [0x40, 0x50, 0x60, 0x70, 0x70, 0x70]),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(out.len(), 64 + 64 * i);
            let mut record = name.as_bytes().to_vec();
            record.resize(19, 0);
            record.push(0xff);
            record.extend(start.to_le_bytes());
            words.iter().for_each(|w| record.extend(w.to_le_bytes()));
            record.extend([0xff; 16]);
            out.extend(record);
        }
        out.extend(names);
        out.extend(data);
        out
    }

    #[test]
    fn counts_checks_classes_and_root_groups() {
        let bytes = container();
        let b = Budget::new(1 << 20);
        let mut k = Kind::default();
        payload(&mut k, &bytes, 2, "a.nif", 0, &b).unwrap();
        payload(&mut k, &bytes, 1, "b.nif", 0, &b).unwrap();
        assert!(payload(&mut k, &bytes[..100], 1, "c.nif", 0, &b).is_err());
        assert_eq!((k.payloads, k.read), (3, 2));
        assert_eq!(k.errors.get("TruncatedHavokContainer"), Some(&1));
        let c = k.checks["eight_zero_bytes_at_every_pointer_source"];
        assert_eq!((c.holds, c.fails), (2, 0));
        let c = k.checks["record_starts_read_big_endian_in_bounds"];
        assert_eq!((c.holds, c.fails), (0, 3));
        let root = k.root_counts["Root|+024"];
        assert_eq!((root.holds, root.fails), (1, 1));
        let group = k.root_groups["Root|+016|next_u32_is_0x80000000_or_count"];
        assert_eq!((group.holds, group.fails), (2, 0));
        let group = k.root_groups["Root|+032|next_u32_is_0x80000000_or_count"];
        assert_eq!((group.holds, group.fails), (0, 2));
        let group = k.root_groups["Root|+016|pointer_source_at_group_start_iff_count_nonzero"];
        assert_eq!((group.holds, group.fails), (2, 0));
        let class = &k.classes["Root"];
        assert_eq!((class.listed, class.payloads, class.objects), (2, 2, 2));
        assert_eq!(class.objects_eq_system_objects.holds, 1);
        let mut s = Summary::default();
        s.kinds.insert("bhkPhysicsSystem".into(), k);
        let text = tsv(&s);
        assert!(
            text.contains("havok.bhkPhysicsSystem.read\tread\t2\n"),
            "{text}"
        );
        assert!(text.contains(
            "havok.bhkPhysicsSystem.class\tRoot\tlisted=2 payloads=2 objects=2 signatures=00000007x2 count_eq_system_objects=holds=1 fails=1\n"
        ));
        assert!(text.contains("havok.bhkPhysicsSystem.fill\tclassnames|06\t2\n"));
    }
}
