//! Collision census: counts and small stored values only, no physics semantics.
//!
//! Walks every winning NIF of the mounted VFS, reads the collision blocks with
//! the typed view in `fo4_nif::collision`, and measures the opaque bhkSystem
//! payloads without interpreting them. Then joins winning records that cite a
//! model (MODL) with the per-model result.
use crate::Reports;
use fo4_core::{add, Budget, Manifest, Result};
use fo4_nif::{Nif, SystemKind, Value};
use fo4_vfs::Vfs;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

const CAP: usize = 256;
const WORD_OFFSETS: usize = 64;
const OTHER: &str = "<other>";

fn bump(map: &mut BTreeMap<String, u64>, key: &str, cap: usize) {
    let key = if map.contains_key(key) || map.len() < cap {
        key
    } else {
        OTHER
    };
    *map.entry(key.to_owned()).or_default() += 1;
}
fn bucket(n: u64) -> String {
    match n {
        0..=9 => format!("{n}"),
        10..=99 => "10-99".into(),
        _ => "100+".into(),
    }
}
fn size_bucket(n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let k = 63 - n.leading_zeros();
    format!("2^{k:02}..2^{:02}", k + 1)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if (0x20..0x7f).contains(b) {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}

#[derive(Default, Serialize)]
pub struct WordStat {
    /// Distinct u32 LE values seen at this offset (saturates at `CAP`).
    pub distinct: u64,
    pub saturated: bool,
    pub present: u64,
    pub values: BTreeMap<u32, u64>,
}
#[derive(Default, Serialize)]
pub struct Run {
    pub payloads: u64,
    pub first_offset_min: u64,
    pub first_offset_max: u64,
}
#[derive(Default, Serialize)]
pub struct Payload {
    pub count: u64,
    pub bytes: u64,
    pub min: u64,
    pub p10: u64,
    pub median: u64,
    pub p90: u64,
    pub max: u64,
    pub empty: u64,
    pub distinct_sha256: u64,
    /// Payloads whose bytes occur in more than one block of the census.
    pub shared_blocks: u64,
    pub block_tail_nonzero: u64,
    /// Length of the byte prefix shared by every payload of this kind.
    pub common_prefix: u64,
    pub sizes: BTreeMap<String, u64>,
    pub prefix4: BTreeMap<String, u64>,
    pub prefix16: BTreeMap<String, u64>,
    pub words: Vec<WordStat>,
    /// "offset|endian|cleared_top_bits" -> (payload length - masked word) ->
    /// payloads. A field that tracks the payload length shows one dominant
    /// difference. Saturates at 64 differences per key.
    pub length_minus_word: BTreeMap<String, BTreeMap<i64, u64>>,
    /// "a+b" (aligned offsets) -> payloads where u32le(a) + u32le(b) equals
    /// the payload length.
    pub word_pairs_sum_to_length: BTreeMap<String, u64>,
    /// NUL-terminated printable ASCII runs (4..=64 bytes) -> payloads containing them.
    pub runs: BTreeMap<String, Run>,
    pub runs_saturated: bool,
    #[serde(skip)]
    all_sizes: Vec<u64>,
    #[serde(skip)]
    hashes: HashMap<String, u64>,
    #[serde(skip)]
    first: Vec<u8>,
    #[serde(skip)]
    diffs: Vec<BTreeMap<i64, u64>>,
    #[serde(skip)]
    pairs: Vec<u64>,
}
const PREFIX_WINDOW: usize = 4096;
const RUN_MAX: usize = 64;
impl Payload {
    fn add(&mut self, bytes: &[u8], tail: usize, b: &Budget) -> Result<String> {
        let len = bytes.len() as u64;
        if self.count == 0 {
            let n = bytes.len().min(PREFIX_WINDOW);
            b.reserve(&mut self.first, n, "collision census prefix", 0)?;
            self.first.extend_from_slice(&bytes[..n]);
            self.common_prefix = n as u64;
        } else {
            let same = self
                .first
                .iter()
                .zip(bytes)
                .take_while(|(a, b)| a == b)
                .count() as u64;
            self.common_prefix = self.common_prefix.min(same);
        }
        self.count += 1;
        self.bytes = add(self.bytes, len, "collision census", 0)?;
        if len == 0 {
            self.empty += 1;
        }
        if tail != 0 {
            self.block_tail_nonzero += 1;
        }
        b.reserve(&mut self.all_sizes, 1, "collision census", 0)?;
        self.all_sizes.push(len);
        bump(&mut self.sizes, &size_bucket(len), CAP);
        let sha = fo4_core::digest_bytes(bytes);
        if !self.hashes.contains_key(&sha) {
            b.charge(160, "collision census hashes", 0)?;
        }
        *self.hashes.entry(sha.clone()).or_default() += 1;
        bump(&mut self.prefix4, &hex(&bytes[..bytes.len().min(4)]), 64);
        let p16 = &bytes[..bytes.len().min(16)];
        bump(
            &mut self.prefix16,
            &format!("{} {}", hex(p16), ascii(p16)),
            64,
        );
        if self.words.is_empty() {
            self.words.resize_with(WORD_OFFSETS, WordStat::default);
        }
        for (i, stat) in self.words.iter_mut().enumerate() {
            let off = i * 4;
            let Some(w) = bytes.get(off..off + 4) else {
                break;
            };
            let le = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            let be = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
            stat.present += 1;
            if stat.values.contains_key(&le) || stat.values.len() < CAP {
                if !stat.values.contains_key(&le) {
                    stat.distinct += 1;
                }
                *stat.values.entry(le).or_default() += 1;
            } else {
                stat.saturated = true;
            }
            if self.diffs.is_empty() {
                self.diffs.resize_with(WORD_OFFSETS * 18, BTreeMap::new);
            }
            for (e, word) in [le, be].into_iter().enumerate() {
                for k in 0..=8u32 {
                    let masked = (word & (u32::MAX >> k)) as i64;
                    let diffs = &mut self.diffs[(i * 2 + e) * 9 + k as usize];
                    let diff = len as i64 - masked;
                    if diffs.contains_key(&diff) || diffs.len() < 64 {
                        *diffs.entry(diff).or_default() += 1;
                    }
                }
            }
        }
        let mut words = [0u64; WORD_OFFSETS];
        let n = (bytes.len() / 4).min(WORD_OFFSETS);
        for (i, w) in bytes.as_chunks::<4>().0.iter().take(n).enumerate() {
            words[i] = u32::from_le_bytes(*w) as u64;
        }
        if self.pairs.is_empty() {
            self.pairs.resize(WORD_OFFSETS * WORD_OFFSETS, 0);
        }
        for a in 0..n {
            for c in a + 1..n {
                if words[a] + words[c] == len {
                    self.pairs[a * WORD_OFFSETS + c] += 1;
                }
            }
        }
        let mut seen: HashMap<&[u8], u64> = HashMap::new();
        let mut start = 0;
        for (i, byte) in bytes.iter().enumerate() {
            if (0x20..0x7f).contains(byte) {
                continue;
            }
            let run = &bytes[start..i];
            if *byte == 0 && (4..=RUN_MAX).contains(&run.len()) && seen.len() < 4096 {
                seen.entry(run).or_insert(start as u64);
            }
            start = i + 1;
        }
        for (run, offset) in seen {
            let key = ascii(run);
            let full = self.runs.len() >= 65536;
            match self.runs.get_mut(&key) {
                Some(r) => {
                    r.payloads += 1;
                    r.first_offset_min = r.first_offset_min.min(offset);
                    r.first_offset_max = r.first_offset_max.max(offset);
                }
                None if !full => {
                    b.charge(key.len() as u64 + 96, "collision census runs", 0)?;
                    self.runs.insert(
                        key,
                        Run {
                            payloads: 1,
                            first_offset_min: offset,
                            first_offset_max: offset,
                        },
                    );
                }
                None => self.runs_saturated = true,
            }
        }
        Ok(sha)
    }
    fn finish(&mut self) {
        self.all_sizes.sort_unstable();
        let q = |p: usize| -> u64 {
            if self.all_sizes.is_empty() {
                0
            } else {
                self.all_sizes[(self.all_sizes.len() - 1) * p / 100]
            }
        };
        self.min = q(0);
        self.p10 = q(10);
        self.median = q(50);
        self.p90 = q(90);
        self.max = q(100);
        for (slot, diffs) in std::mem::take(&mut self.diffs).into_iter().enumerate() {
            if diffs.is_empty() {
                continue;
            }
            let (off, e, k) = (slot / 18 * 4, slot / 9 % 2, slot % 9);
            let endian = if e == 0 { "le" } else { "be" };
            self.length_minus_word
                .insert(format!("{off:03}|{endian}|{k}"), diffs);
        }
        for (slot, n) in std::mem::take(&mut self.pairs).into_iter().enumerate() {
            if n > 0 {
                let (a, c) = (slot / WORD_OFFSETS * 4, slot % WORD_OFFSETS * 4);
                self.word_pairs_sum_to_length
                    .insert(format!("{a:03}+{c:03}"), n);
            }
        }
        self.distinct_sha256 = self.hashes.len() as u64;
        self.shared_blocks = self.hashes.values().filter(|n| **n > 1).sum();
    }
}

#[derive(Default, Serialize)]
pub struct Records {
    pub records: u64,
    pub no_model: u64,
    pub model_unresolved: u64,
    pub model_absent: u64,
    pub model_failed: u64,
    pub physics: u64,
    pub ragdoll_only: u64,
    pub object_without_system: u64,
    pub none: u64,
}

#[derive(Default, Serialize)]
pub struct Summary {
    pub nif_files: u64,
    pub nif_read: u64,
    pub nif_failed: u64,
    pub collision_view_failed: u64,
    pub files_with_object: u64,
    pub files_with_system: u64,
    pub files_without_collision: u64,
    pub files_with_ragdoll_only: u64,
    /// Collision-related block types: block count and file count.
    pub block_types: BTreeMap<String, u64>,
    pub block_type_files: BTreeMap<String, u64>,
    /// Every block type that stayed undecoded in the whole census.
    pub undecoded_types: BTreeMap<String, u64>,
    pub objects_per_file: BTreeMap<String, u64>,
    pub systems_per_file: BTreeMap<String, u64>,
    pub object_flags: BTreeMap<String, u64>,
    pub object_flag_bits: BTreeMap<String, u64>,
    pub body_ids: BTreeMap<String, u64>,
    pub data_types: BTreeMap<String, u64>,
    pub target_types: BTreeMap<String, u64>,
    pub target_relation: BTreeMap<String, u64>,
    pub target_is_root: BTreeMap<String, u64>,
    pub owners_per_object: BTreeMap<String, u64>,
    pub owner_types: BTreeMap<String, u64>,
    pub owner_link_types: BTreeMap<String, u64>,
    pub owner_target_agreement: BTreeMap<String, u64>,
    pub objects_per_system: BTreeMap<String, u64>,
    pub body_id_vs_system: BTreeMap<String, u64>,
    pub payloads: BTreeMap<String, Payload>,
    pub records: BTreeMap<String, Records>,
    pub records_scanned: u64,
    pub records_failed: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Physics,
    RagdollOnly,
    ObjectWithoutSystem,
    None,
    Failed,
}

fn related(ty: &str) -> bool {
    ty.starts_with("bhk")
        || ty.contains("Collision")
        || ty.contains("Physics")
        || ty.contains("Ragdoll")
}

fn fields<'n>(n: &'n Nif<'_>, i: usize) -> Option<&'n [fo4_nif::Field]> {
    match n.blocks.get(i)?.decoded.as_ref()? {
        fo4_nif::Data::Node { fields, .. } | fo4_nif::Data::Fields(fields) => Some(fields),
        fo4_nif::Data::Mesh(m) => Some(&m.fields),
    }
}

fn nif(s: &mut Summary, n: &Nif<'_>, file: &str, b: &Budget, r: &mut Reports) -> Result<Status> {
    let mut present: BTreeMap<&str, ()> = BTreeMap::new();
    for block in &n.blocks {
        let ty = n.block_types[block.type_index as usize].as_str();
        if block.decoded.is_none() {
            bump(&mut s.undecoded_types, ty, 4096);
        }
        if related(ty) {
            bump(&mut s.block_types, ty, 4096);
            present.insert(ty, ());
        }
    }
    for ty in present.keys() {
        bump(&mut s.block_type_files, ty, 4096);
    }
    let c = match n.collision(file, b) {
        Ok(c) => c,
        Err(e) => {
            s.collision_view_failed += 1;
            r.error(&e)?;
            return Ok(Status::Failed);
        }
    };
    let ty = |i: i32| -> &str {
        if i < 0 {
            "null"
        } else {
            &n.block_types[n.blocks[i as usize].type_index as usize]
        }
    };
    bump(
        &mut s.objects_per_file,
        &bucket(c.objects.len() as u64),
        CAP,
    );
    bump(
        &mut s.systems_per_file,
        &bucket(c.systems.len() as u64),
        CAP,
    );
    if !c.objects.is_empty() {
        s.files_with_object += 1;
    }
    if !c.systems.is_empty() {
        s.files_with_system += 1;
    }
    for o in &c.objects {
        bump(&mut s.object_flags, &format!("0x{:04x}", o.flags), CAP);
        for bit in 0..16 {
            if o.flags & (1 << bit) != 0 {
                bump(&mut s.object_flag_bits, &format!("bit{bit:02}"), CAP);
            }
        }
        bump(&mut s.body_ids, &format!("{}", o.body_id), 64);
        bump(&mut s.data_types, ty(o.data), CAP);
        bump(&mut s.target_types, ty(o.target), CAP);
        let relation = if o.target < 0 {
            "target_null"
        } else {
            match fields(n, o.target as usize) {
                None => "target_undecoded",
                Some(f) => match f.iter().find(|f| f.name == "Collision") {
                    None => "target_without_collision_field",
                    Some(f) if matches!(f.value, Value::Ref(v) if v == o.block as i32) => {
                        "target_links_back"
                    }
                    Some(_) => "target_links_elsewhere",
                },
            }
        };
        bump(&mut s.target_relation, relation, CAP);
        let root = o.target >= 0 && n.roots.contains(&o.target);
        bump(
            &mut s.target_is_root,
            if root { "root" } else { "not_root" },
            CAP,
        );
        let owners = c
            .owners
            .iter()
            .filter(|w| w.collision == o.block as i32)
            .count() as u64;
        bump(&mut s.owners_per_object, &bucket(owners), CAP);
        if let Some(system) = c.system(o.data) {
            let on_system: Vec<_> = c.objects.iter().filter(|x| x.data == o.data).collect();
            let k = on_system.len() as u64;
            let dup = on_system.iter().filter(|x| x.body_id == o.body_id).count() > 1;
            let rel = if (o.body_id as u64) < k {
                "body_id_lt_objects_on_system"
            } else {
                "body_id_ge_objects_on_system"
            };
            bump(
                &mut s.body_id_vs_system,
                &format!("{}|{rel}", system.kind.block_type()),
                CAP,
            );
            if dup {
                bump(
                    &mut s.body_id_vs_system,
                    &format!("{}|body_id_repeated_on_system", system.kind.block_type()),
                    CAP,
                );
            }
        }
    }
    for w in &c.owners {
        bump(&mut s.owner_types, ty(w.block as i32), CAP);
        bump(&mut s.owner_link_types, ty(w.collision), CAP);
        let agreement = match c.objects.iter().find(|o| o.block as i32 == w.collision) {
            None => "link_not_to_bhkNPCollisionObject",
            Some(o) if o.target == w.block as i32 => "object_target_is_owner",
            Some(_) => "object_target_differs",
        };
        bump(&mut s.owner_target_agreement, agreement, CAP);
    }
    let mut systems = Vec::new();
    for system in &c.systems {
        let users = c
            .objects
            .iter()
            .filter(|o| o.data == system.block as i32)
            .count() as u64;
        bump(
            &mut s.objects_per_system,
            &format!("{}|{}", system.kind.block_type(), bucket(users)),
            CAP,
        );
        let stats = s
            .payloads
            .entry(system.kind.block_type().to_owned())
            .or_default();
        let sha = stats.add(system.payload, system.tail, b)?;
        systems.push(serde_json::json!({
            "kind": system.kind.block_type(),
            "block": system.block,
            "payload_offset": system.payload_offset,
            "size": system.payload.len(),
            "sha256": sha,
            "objects": users,
        }));
    }
    let status = if c.systems.iter().any(|x| x.kind == SystemKind::Physics) {
        Status::Physics
    } else if !c.systems.is_empty() {
        s.files_with_ragdoll_only += 1;
        Status::RagdollOnly
    } else if !c.objects.is_empty() {
        Status::ObjectWithoutSystem
    } else {
        s.files_without_collision += 1;
        Status::None
    };
    if !c.is_empty() {
        r.item(&serde_json::json!({
            "kind": "nif_collision",
            "path": file,
            "objects": c.objects,
            "owners": c.owners.len(),
            "systems": systems,
        }))?;
    }
    Ok(status)
}

const SKIPPED: [&[u8; 4]; 15] = [
    b"REFR", b"ACHR", b"PGRE", b"PMIS", b"PHZD", b"PARW", b"PBAR", b"PBEA", b"PCON", b"PFLA",
    b"NAVM", b"LAND", b"INFO", b"CELL", b"WRLD",
];

pub fn sweep(m: &Manifest, r: &mut Reports) -> Result<()> {
    r.items_enabled = true;
    let v = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for e in &v.diagnostics {
        r.error(e)?;
    }
    v.check_complete()?;
    r.summary.vfs_memory = v.memory_used();
    r.summary.fingerprint = Some(v.fingerprint()?.to_owned());
    r.summary.fingerprint_scope = Some("assets".into());
    let aggregate = Budget::new(m.limits.max_memory);
    let mut s = Summary::default();
    let mut status: HashMap<Vec<u8>, Status> = HashMap::new();
    for chain in v.candidates.chunk_by(|a, b| a.key == b.key) {
        let Some(c) = chain.last() else { continue };
        let Some(key) = &c.key else { continue };
        if !key.ends_with(b".nif") {
            continue;
        }
        s.nif_files += 1;
        let work = Budget::new(m.limits.max_memory);
        let result = v.read_candidate(c, &work).and_then(|bytes| {
            let n = Nif::read(&bytes, &c.original, m.game, &m.limits, &work)?;
            nif(&mut s, &n, &c.original, &work, r)
        });
        let st = match result {
            Ok(st) => {
                s.nif_read += 1;
                st
            }
            Err(e) => {
                s.nif_failed += 1;
                r.error(&e)?;
                Status::Failed
            }
        };
        aggregate.charge(key.len() as u64 + 64, "collision model index", 0)?;
        status.insert(key.clone(), st);
    }
    for p in s.payloads.values_mut() {
        p.finish();
    }
    let index = Budget::new(m.limits.max_override_memory);
    let store = fo4_plugin::typed::Store::open(m, &index)?;
    for e in &store.diagnostics {
        r.error(e)?;
    }
    for winner in store.winners() {
        if SKIPPED.contains(&&winner.signature) || winner.deleted {
            continue;
        }
        let key = store.overrides().form_key(winner.key, store.order());
        let work = Budget::new(m.limits.max_memory);
        let attempt = (|| -> Result<Option<(Option<String>, String)>> {
            let Some(record) = store.raw(&key, &work)? else {
                return Ok(None);
            };
            let fields = record.fields(&m.limits, &work)?;
            let mut model = None;
            let mut editor = None;
            for f in fields {
                if let fo4_plugin::Value::Text(t) = f.value {
                    match &f.signature {
                        b"MODL" if model.is_none() => model = Some(t),
                        b"EDID" if editor.is_none() => editor = Some(t),
                        _ => {}
                    }
                }
            }
            Ok(model.map(|m| (editor, m)))
        })();
        s.records_scanned += 1;
        let sig = String::from_utf8_lossy(&winner.signature).into_owned();
        let found = match attempt {
            Ok(found) => found,
            Err(e) => {
                s.records_failed += 1;
                r.error(&e)?;
                continue;
            }
        };
        let counts = s.records.entry(sig.clone()).or_default();
        counts.records += 1;
        let Some((editor, model)) = found else {
            counts.no_model += 1;
            continue;
        };
        if model.is_empty() {
            counts.no_model += 1;
            continue;
        }
        let label = match fo4_scene::paths::resolve_with(
            fo4_scene::paths::Citation::RecordModel,
            model.as_bytes(),
            &v,
            fo4_scene::RuleLevel::Measured,
            &work,
        ) {
            Err(e) => {
                // Rejected or unresolvable citation: counted, never repaired.
                counts.model_unresolved += 1;
                if e.code != "RejectedVfsPath" && e.code != "MissingPath" {
                    r.notice(&e)?;
                }
                "model_unresolved"
            }
            Ok(resolved) => match status.get(&resolved.key) {
                None => {
                    counts.model_absent += 1;
                    "model_absent"
                }
                Some(Status::Failed) => {
                    counts.model_failed += 1;
                    "model_failed"
                }
                Some(Status::Physics) => {
                    counts.physics += 1;
                    continue;
                }
                Some(Status::RagdollOnly) => {
                    counts.ragdoll_only += 1;
                    "ragdoll_only"
                }
                Some(Status::ObjectWithoutSystem) => {
                    counts.object_without_system += 1;
                    "object_without_system"
                }
                Some(Status::None) => {
                    counts.none += 1;
                    "none"
                }
            },
        };
        r.item(&serde_json::json!({
            "kind": "record_without_physics_system",
            "signature": sig,
            "form": key,
            "editor_id": editor,
            "model": model,
            "status": label,
        }))?;
    }
    r.summary.other_memory = aggregate.used();
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
    r.tsv = Some(("collision.tsv", text));
    r.summary.collision = s;
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
        ("files_with_bhkNPCollisionObject", s.files_with_object),
        ("files_with_bhkSystem", s.files_with_system),
        ("files_with_ragdoll_only", s.files_with_ragdoll_only),
        ("files_without_collision", s.files_without_collision),
        ("records_scanned", s.records_scanned),
        ("records_failed", s.records_failed),
    ] {
        row("total", k, v.to_string());
    }
    for (section, map) in [
        ("block_type.blocks", &s.block_types),
        ("block_type.files", &s.block_type_files),
        ("undecoded_type.blocks", &s.undecoded_types),
        ("objects_per_file", &s.objects_per_file),
        ("systems_per_file", &s.systems_per_file),
        ("object.flags", &s.object_flags),
        ("object.flag_bits", &s.object_flag_bits),
        ("object.body_id", &s.body_ids),
        ("object.data_type", &s.data_types),
        ("object.target_type", &s.target_types),
        ("object.target_relation", &s.target_relation),
        ("object.target_is_root", &s.target_is_root),
        ("object.owners", &s.owners_per_object),
        ("owner.type", &s.owner_types),
        ("owner.link_type", &s.owner_link_types),
        ("owner.agreement", &s.owner_target_agreement),
        ("system.objects", &s.objects_per_system),
        ("system.body_id", &s.body_id_vs_system),
    ] {
        for (k, v) in map {
            row(section, k, v.to_string());
        }
    }
    for (kind, p) in &s.payloads {
        let sec = |name: &str| format!("payload.{kind}.{name}");
        for (k, v) in [
            ("count", p.count),
            ("bytes", p.bytes),
            ("min", p.min),
            ("p10", p.p10),
            ("median", p.median),
            ("p90", p.p90),
            ("max", p.max),
            ("empty", p.empty),
            ("distinct_sha256", p.distinct_sha256),
            ("shared_blocks", p.shared_blocks),
            ("block_tail_nonzero", p.block_tail_nonzero),
        ] {
            row(&sec("stats"), k, v.to_string());
        }
        for (k, v) in &p.sizes {
            row(&sec("size"), k, v.to_string());
        }
        for (k, v) in &p.prefix4 {
            row(&sec("prefix4"), k, v.to_string());
        }
        for (k, v) in &p.prefix16 {
            row(&sec("prefix16"), k, v.to_string());
        }
        for (i, w) in p.words.iter().enumerate() {
            let mut top: Vec<_> = w.values.iter().collect();
            top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let shown: Vec<String> = top
                .iter()
                .take(6)
                .map(|(v, n)| format!("{v:08x}x{n}"))
                .collect();
            row(
                &sec("u32le"),
                &format!("{:02}", i * 4),
                format!(
                    "present={} distinct={}{} top={}",
                    w.present,
                    w.distinct,
                    if w.saturated { "+" } else { "" },
                    shown.join(",")
                ),
            );
        }
        row(&sec("stats"), "common_prefix", p.common_prefix.to_string());
        // A word tracks the length when it varies and one difference dominates.
        let half = p.count.div_ceil(2).max(1);
        let mut shown = std::collections::BTreeSet::new();
        for (k, diffs) in &p.length_minus_word {
            // Keys sort by cleared bits last: report only the fewest cleared bits.
            if shown.contains(&k[..6]) {
                continue;
            }
            let offset: usize = k[..3].parse().unwrap_or(0);
            let Some(word) = p.words.get(offset / 4) else {
                continue;
            };
            if word.distinct == 1 {
                continue;
            }
            // One difference must hold for at least 99% of the payloads that reach the word.
            let floor = (word.present * 99).div_ceil(100).max(1);
            if let Some((d, n)) = diffs.iter().max_by_key(|(_, n)| **n) {
                if *n >= floor {
                    shown.insert(&k[..6]);
                    row(&sec("length_minus_word"), k, format!("{d}x{n}"));
                }
            }
        }
        for (k, n) in &p.word_pairs_sum_to_length {
            let both_constant = k
                .split('+')
                .filter_map(|o| o.parse::<usize>().ok())
                .all(|o| p.words.get(o / 4).is_some_and(|w| w.distinct == 1));
            if !both_constant && *n >= half {
                row(&sec("word_pair_sum_is_length"), k, n.to_string());
            }
        }
        for (k, r) in p.runs.iter().filter(|(_, r)| r.payloads >= half) {
            row(
                &sec("run_in_half"),
                k,
                format!(
                    "payloads={} first_offset={}..{}",
                    r.payloads, r.first_offset_min, r.first_offset_max
                ),
            );
        }
    }
    for (sig, c) in &s.records {
        if c.records == c.no_model {
            continue;
        }
        row(
            "records",
            sig,
            format!(
                "records={} no_model={} physics={} ragdoll_only={} object_without_system={} none={} model_unresolved={} model_absent={} model_failed={}",
                c.records,
                c.no_model,
                c.physics,
                c.ragdoll_only,
                c.object_without_system,
                c.none,
                c.model_unresolved,
                c.model_absent,
                c.model_failed
            ),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_measures_are_neutral_counts() {
        let mut p = Payload::default();
        let b = Budget::new(1 << 20);
        // Synthetic: a big-endian word holding the length with the top bit set,
        // then a NUL-terminated name and filler of varying size.
        let make = |filler: usize| {
            let len = (4 + 6 + filler) as u32;
            let mut v = (0x8000_0000 | len).to_be_bytes().to_vec();
            v.extend(b"ABCDE\0");
            v.extend(vec![1u8; filler]);
            v
        };
        let small = make(6);
        let large = make(12);
        p.add(&small, 0, &b).unwrap();
        p.add(&small, 3, &b).unwrap();
        p.add(&large, 0, &b).unwrap();
        p.add(&[], 0, &b).unwrap();
        p.finish();
        assert_eq!((p.count, p.empty, p.block_tail_nonzero), (4, 1, 1));
        assert_eq!((p.min, p.max), (0, 22));
        assert_eq!((p.distinct_sha256, p.shared_blocks), (3, 2));
        assert_eq!(p.common_prefix, 0);
        assert_eq!(p.length_minus_word["000|be|1"].get(&0), Some(&3));
        assert_eq!(p.length_minus_word["000|be|0"].get(&0), None);
        let run = &p.runs["ABCDE"];
        assert_eq!(
            (run.payloads, run.first_offset_min, run.first_offset_max),
            (3, 4, 4)
        );
        assert_eq!(
            p.words[1].values.get(&u32::from_le_bytes(*b"ABCD")),
            Some(&3)
        );
        let text = {
            let mut s = Summary::default();
            s.payloads.insert("bhkPhysicsSystem".into(), p);
            tsv(&s)
        };
        assert!(
            text.contains("payload.bhkPhysicsSystem.length_minus_word\t000|be|0\t-2147483648x3\n"),
            "{text}"
        );
        assert!(
            !text.contains("length_minus_word\t000|be|1"),
            "fewest cleared bits only"
        );
        assert!(
            !text.contains("length_minus_word\t004|"),
            "constant words are skipped"
        );
        assert!(text.contains(
            "payload.bhkPhysicsSystem.run_in_half\tABCDE\tpayloads=3 first_offset=4..4\n"
        ));
        assert!(text.contains("payload.bhkPhysicsSystem.prefix4\t80000010\t2\n"));
    }
    #[test]
    fn common_prefix_is_the_shared_leading_bytes() {
        let mut p = Payload::default();
        let b = Budget::new(1 << 20);
        p.add(b"WXYZ0123", 0, &b).unwrap();
        p.add(b"WXYZ01", 0, &b).unwrap();
        p.add(b"WXYQ", 0, &b).unwrap();
        assert_eq!(p.common_prefix, 3);
    }
    #[test]
    fn word_pairs_that_sum_to_the_length_are_counted() {
        let mut p = Payload::default();
        let b = Budget::new(1 << 20);
        for filler in [0usize, 4, 8] {
            let len = 12 + filler as u32;
            let mut v = Vec::new();
            for w in [5u32, len - 5, 77] {
                v.extend(w.to_le_bytes());
            }
            v.extend(vec![0; filler]);
            p.add(&v, 0, &b).unwrap();
        }
        p.finish();
        assert_eq!(p.word_pairs_sum_to_length.get("000+004"), Some(&3));
        assert_eq!(p.word_pairs_sum_to_length.get("000+008"), None);
        let mut s = Summary::default();
        s.payloads.insert("bhkPhysicsSystem".into(), p);
        assert!(tsv(&s).contains("payload.bhkPhysicsSystem.word_pair_sum_is_length\t000+004\t3\n"));
    }
    #[test]
    fn caps_fold_into_other() {
        let mut m = BTreeMap::new();
        for i in 0..5 {
            bump(&mut m, &i.to_string(), 3);
        }
        bump(&mut m, "0", 3);
        assert_eq!(m.get("0"), Some(&2));
        assert_eq!(m.get(OTHER), Some(&2));
        assert_eq!(size_bucket(1), "2^00..2^01");
        assert_eq!(size_bucket(4096), "2^12..2^13");
        assert_eq!(bucket(150), "100+");
    }
}
