#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop)]
//! Measurements behind the actor record layouts of `spec/actor.md`, over
//! every winning record of the load order. Writes a plain-text report.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey, Record};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    path::Path,
};

struct Rec {
    signature: [u8; 4],
    fields: Vec<([u8; 4], Vec<u8>)>,
    plugin: usize,
    file: String,
}
impl Rec {
    fn all<'a>(&'a self, s: &'a [u8; 4]) -> impl Iterator<Item = &'a Vec<u8>> {
        self.fields.iter().filter(move |f| &f.0 == s).map(|f| &f.1)
    }
    fn one(&self, s: &[u8; 4]) -> Option<&Vec<u8>> {
        self.fields.iter().find(|f| &f.0 == s).map(|f| &f.1)
    }
}
struct Db<'a> {
    store: &'a Store,
    cache: HashMap<(String, u32), Option<std::rc::Rc<Rec>>>,
}
impl Db<'_> {
    fn get(&mut self, key: &FormKey) -> Option<std::rc::Rc<Rec>> {
        let k = (key.plugin.to_lowercase(), key.object_id);
        if let Some(v) = self.cache.get(&k) {
            return v.clone();
        }
        let b = Budget::new(256 << 20);
        let w = self.store.overrides().winner(key);
        let v = w.and_then(|w| {
            let plugin = w.plugin;
            self.store.raw(key, &b).ok().flatten().map(|r: Record| {
                std::rc::Rc::new(Rec {
                    signature: r.signature,
                    fields: r
                        .subrecords
                        .iter()
                        .map(|s| (s.signature, r.data(s).to_vec()))
                        .collect(),
                    plugin,
                    file: r.source.file.clone(),
                })
            })
        });
        self.cache.insert(k, v.clone());
        v
    }
    fn reference(&mut self, from: &Rec, id: u32) -> Option<(FormKey, std::rc::Rc<Rec>)> {
        if id == 0 {
            return None;
        }
        let key = self
            .store
            .order()
            .remap(from.plugin, id, &from.file, 0)
            .ok()
            .flatten()?;
        let r = self.get(&key)?;
        Some((key, r))
    }
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
}
fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2)
        .map(|v| u16::from_le_bytes(v.try_into().unwrap()))
}
fn f32_at(d: &[u8], o: usize) -> Option<f32> {
    u32_at(d, o).map(f32::from_bits)
}
fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}
fn keys(store: &Store, sig: &[u8; 4]) -> Vec<FormKey> {
    store
        .winners()
        .filter(|w| w.signature == *sig && !w.deleted)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect()
}
fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("actor_census MANIFEST OUT.txt".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut db = Db {
        store: &store,
        cache: HashMap::new(),
    };
    let mut out = String::new();
    npc(&store, &mut db, &mut out)?;
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    chain(&store, &mut db, &mut out)?;
    rest(&store, &mut db, &vfs, &m, &mut out)?;
    placed(&store, &vfs, &m, &mut out)?;
    std::fs::write(&args[1], out)?;
    Ok(())
}

fn npc(store: &Store, db: &mut Db<'_>, out: &mut String) -> Result<(), Box<dyn std::error::Error>> {
    let npcs = keys(store, b"NPC_");
    // N1: ACBS u16 at 14 against TPTA slots.
    let mut agree = [[0u64; 4]; 16];
    let mut tpta_records = 0;
    let mut no_tpta_flags: BTreeMap<u16, u64> = BTreeMap::new();
    let mut tplt_vs_slots = [0u64; 3];
    for k in &npcs {
        let Some(r) = db.get(k) else { continue };
        let Some(acbs) = r.one(b"ACBS") else { continue };
        let tf = u16_at(acbs, 14).unwrap_or(0);
        match r.one(b"TPTA") {
            Some(t) => {
                tpta_records += 1;
                let tplt = r.one(b"TPLT").and_then(|v| u32_at(v, 0));
                for bit in 0..16 {
                    let set = tf & (1 << bit) != 0;
                    let nonzero = u32_at(t, bit * 4).is_some_and(|v| v != 0);
                    agree[bit][(set as usize) * 2 + nonzero as usize] += 1;
                    if nonzero {
                        let v = u32_at(t, bit * 4);
                        if tplt.is_some() && v == tplt {
                            tplt_vs_slots[0] += 1;
                        } else {
                            tplt_vs_slots[1] += 1;
                        }
                    }
                }
            }
            None => bump(&mut no_tpta_flags, tf),
        }
        if r.one(b"TPLT").is_none() && tf != 0 {
            tplt_vs_slots[2] += 1;
        }
    }
    writeln!(
        out,
        "## N1 ACBS u16@14 bit k vs TPTA slot k nonzero ({tpta_records} NPC_ with TPTA)"
    )?;
    writeln!(out, "bit\tclear&zero\tclear&nonzero\tset&zero\tset&nonzero")?;
    for (bit, a) in agree.iter().enumerate() {
        writeln!(out, "{bit}\t{}\t{}\t{}\t{}", a[0], a[1], a[2], a[3])?;
    }
    writeln!(
        out,
        "nonzero TPTA slots equal to TPLT {} other {}; records with flags but no TPLT {}",
        tplt_vs_slots[0], tplt_vs_slots[1], tplt_vs_slots[2]
    )?;
    writeln!(out, "u16@14 values of NPC_ without TPTA: {no_tpta_flags:?}")?;

    // N2: ACBS u32@0 bits against the race's VTCK slot holding the NPC_'s VTCK.
    let mut by_bit = [[0u64; 4]; 32];
    let mut matches = [0u64; 3];
    for k in &npcs {
        let Some(r) = db.get(k) else { continue };
        let Some(acbs) = r.one(b"ACBS") else { continue };
        if u16_at(acbs, 14).unwrap_or(0) != 0 {
            continue;
        }
        let Some(v) = r.one(b"VTCK").and_then(|v| u32_at(v, 0)) else {
            continue;
        };
        let Some((vk, _)) = db.reference(&r, v) else {
            continue;
        };
        let Some(race) = r.one(b"RNAM").and_then(|v| u32_at(v, 0)) else {
            continue;
        };
        let Some((_, race)) = db.reference(&r, race) else {
            continue;
        };
        let Some(rv) = race.one(b"VTCK").cloned() else {
            continue;
        };
        let s0 = db
            .reference(&race, u32_at(&rv, 0).unwrap_or(0))
            .map(|x| x.0);
        let s1 = db
            .reference(&race, u32_at(&rv, 4).unwrap_or(0))
            .map(|x| x.0);
        let slot = if s0.as_ref() == Some(&vk) && s1.as_ref() == Some(&vk) {
            matches[2] += 1;
            continue;
        } else if s0.as_ref() == Some(&vk) {
            0
        } else if s1.as_ref() == Some(&vk) {
            1
        } else {
            matches[2] += 1;
            continue;
        };
        matches[slot] += 1;
        let flags = u32_at(acbs, 0).unwrap_or(0);
        for (bit, b) in by_bit.iter_mut().enumerate() {
            b[((flags >> bit) & 1) as usize * 2 + slot] += 1;
        }
    }
    writeln!(
        out,
        "\n## N2 untemplated NPC_ VTCK = race VTCK slot 0: {} slot 1: {} neither/both: {}",
        matches[0], matches[1], matches[2]
    )?;
    writeln!(out, "bit\tclear&slot0\tclear&slot1\tset&slot0\tset&slot1")?;
    for (bit, b) in by_bit.iter().enumerate() {
        if b[2] + b[3] > 0 {
            writeln!(out, "{bit}\t{}\t{}\t{}\t{}", b[0], b[1], b[2], b[3])?;
        }
    }

    // N3: own-field presence and equality with the template by template bit.
    let fields: [&[u8; 4]; 8] = [
        b"RNAM", b"WNAM", b"DOFT", b"PNAM", b"VTCK", b"CNTO", b"ANAM", b"FTST",
    ];
    let mut table: BTreeMap<(usize, usize), [u64; 6]> = BTreeMap::new();
    for k in &npcs {
        let Some(r) = db.get(k) else { continue };
        let Some(acbs) = r.one(b"ACBS") else { continue };
        let tf = u16_at(acbs, 14).unwrap_or(0);
        let Some(t) = r.one(b"TPTA").cloned() else {
            continue;
        };
        for (fi, f) in fields.iter().enumerate() {
            for bit in 0..13usize {
                let slot = u32_at(&t, bit * 4).unwrap_or(0);
                let Some((_, tr)) = db.reference(&r, slot) else {
                    continue;
                };
                if tr.signature != *b"NPC_" {
                    continue;
                }
                let own = r.one(f).cloned();
                let theirs = tr.one(f).cloned();
                let set = tf & (1 << bit) != 0;
                let e = table.entry((fi, bit)).or_default();
                let base = if set { 3 } else { 0 };
                e[base] += 1;
                if own.is_some() {
                    e[base + 1] += 1;
                }
                if own.is_some() && own == theirs {
                    e[base + 2] += 1;
                }
            }
        }
    }
    writeln!(
        out,
        "\n## N3 NPC_ with TPTA: own field present / equal to slot's NPC_ value, by bit"
    )?;
    writeln!(
        out,
        "field\tbit\tclear:n\tpresent\tequal\tset:n\tpresent\tequal"
    )?;
    for ((fi, bit), e) in &table {
        writeln!(
            out,
            "{}\t{bit}\t{}\t{}\t{}\t{}\t{}\t{}",
            String::from_utf8_lossy(fields[*fi]),
            e[0],
            e[1],
            e[2],
            e[3],
            e[4],
            e[5]
        )?;
    }
    let _ = (f32_at(&[], 0), text(&[]));
    Ok(())
}
use fo4_nif::{Data, Nif};
use fo4_scene::actor::pose::{self, Affine};
use fo4_scene::transform::Transform;

/// World translations of the named nodes of a file under the static
/// renderer's reading of node rotations (`Transform`), for comparison.
fn static_reading(n: &Nif<'_>, file: &str) -> HashMap<Vec<u8>, [f64; 3]> {
    let mut out = HashMap::new();
    let mut stack: Vec<(usize, Transform, u32)> = n
        .roots
        .iter()
        .filter(|r| **r >= 0)
        .map(|r| (*r as usize, Transform::IDENTITY, 0))
        .collect();
    while let Some((i, parent, depth)) = stack.pop() {
        if depth > 64 {
            continue;
        }
        let Some(Data::Node { fields, children }) =
            n.blocks.get(i).and_then(|b| b.decoded.as_ref())
        else {
            continue;
        };
        let Ok(local) = Transform::from_fields(fields, file) else {
            continue;
        };
        let w = parent.compose(local);
        if let Some(name) = pose::name(n, i) {
            out.entry(name.to_vec()).or_insert(w.translation);
        }
        for c in children.iter().filter(|c| **c >= 0) {
            stack.push((*c as usize, w, depth + 1));
        }
    }
    out
}
struct Skel {
    pose: pose::Skeleton,
    static_reading: HashMap<Vec<u8>, [f64; 3]>,
}
fn read_model(vfs: &fo4_vfs::Vfs, path: &str, b: &Budget) -> Option<(String, Vec<u8>)> {
    let key = fo4_core::path_key(&format!("meshes\\{path}")).ok()?;
    let bytes = vfs.read(&key, b).ok()?;
    Some((key, bytes))
}
fn skeleton(vfs: &fo4_vfs::Vfs, m: &Manifest, path: &str) -> Option<Skel> {
    let b = Budget::new(m.limits.max_memory);
    let (key, bytes) = read_model(vfs, path, &b)?;
    let n = Nif::read(&bytes, &key, fo4_core::Game::Fallout4, &m.limits, &b).ok()?;
    Some(Skel {
        pose: pose::Skeleton::read(&n, &key, &m.limits, &b).ok()?,
        static_reading: static_reading(&n, &key),
    })
}
#[derive(Default, Clone)]
struct Fit {
    shapes: u64,
    all_bones_named: u64,
    missing_bones: u64,
    /// Per shape: composite transforms one translation shared by every
    /// bone within 0.01 / within 0.1 / translations spread / rotated.
    bind: [u64; 4],
    rest: [u64; 4],
    bind_translations: BTreeMap<String, u64>,
    /// Model bone node world translation against the skeleton's, under the
    /// measured reading and under the static renderer's: <0.01, <1, >=1.
    nodes_measured: [u64; 3],
    nodes_static: [u64; 3],
    /// Largest distance a vertex moves from its bind position (stored plus
    /// the bind translation) in the skeleton's rest pose.
    moved: [u64; 6],
}
fn bucket4(m: &[Affine]) -> (usize, [f64; 3]) {
    let mut rot: f64 = 0.;
    let mut spread: f64 = 0.;
    let t0 = m.first().map_or([0.; 3], |a| a.translation);
    for a in m {
        for i in 0..3 {
            for j in 0..3 {
                rot = rot.max((a.scale * a.rows[i][j] - if i == j { 1. } else { 0. }).abs());
            }
            spread = spread.max((a.translation[i] - t0[i]).abs());
        }
    }
    let b = if rot > 1e-3 {
        3
    } else if spread < 0.01 {
        0
    } else if spread < 0.1 {
        1
    } else {
        2
    };
    (b, t0)
}
fn fit(vfs: &fo4_vfs::Vfs, m: &Manifest, model: &str, skel: &Skel, f: &mut Fit) -> Option<()> {
    let b = Budget::new(m.limits.max_memory);
    let (key, bytes) = read_model(vfs, model, &b)?;
    let n = Nif::read(&bytes, &key, fo4_core::Game::Fallout4, &m.limits, &b).ok()?;
    let worlds = pose::node_worlds(&n, &key, &m.limits, &b).ok()?;
    let statics = static_reading(&n, &key);
    for (si, blk) in n.blocks.iter().enumerate() {
        let Some(Data::Mesh(_)) = &blk.decoded else {
            continue;
        };
        let Ok(Some(skin)) = fo4_nif::skin::shape_skin(&n, si, &key, &m.limits, &b) else {
            continue;
        };
        f.shapes += 1;
        let mut missing = 0;
        for bone in &skin.bones {
            let Some(name) = pose::name(&n, bone.node) else {
                missing += 1;
                continue;
            };
            let Some(sw) = skel.pose.bones.get(name) else {
                missing += 1;
                continue;
            };
            let close = |a: [f64; 3], b: [f64; 3]| {
                let d = (0..3).map(|k| (a[k] - b[k]).abs()).fold(0., f64::max);
                if d < 0.01 {
                    0
                } else if d < 1. {
                    1
                } else {
                    2
                }
            };
            if let Some(mw) = worlds.get(&bone.node) {
                f.nodes_measured[close(mw.translation, sw.translation)] += 1;
            }
            if let (Some(mt), Some(st)) = (statics.get(name), skel.static_reading.get(name)) {
                f.nodes_static[close(*mt, *st)] += 1;
            }
        }
        f.missing_bones += missing;
        if missing > 0 {
            continue;
        }
        f.all_bones_named += 1;
        let Ok(bind) = pose::bind_fit(&n, &skin, &key, &m.limits, &b) else {
            continue;
        };
        let mut own = pose::Skeleton::default();
        for bone in &skin.bones {
            if let (Some(name), Some(w)) = (pose::name(&n, bone.node), worlds.get(&bone.node)) {
                own.bones.insert(name.to_vec(), *w);
            }
        }
        let (Ok(bm), Ok(rm)) = (
            pose::bone_matrices(&n, &skin, &own, &key, &b),
            pose::bone_matrices(&n, &skin, &skel.pose, &key, &b),
        ) else {
            continue;
        };
        let (bb, t) = bucket4(&bm);
        f.bind[bb] += 1;
        if bb <= 1 {
            bump(
                &mut f.bind_translations,
                format!("{:.0},{:.0},{:.0}", t[0], t[1], t[2]),
            );
        }
        f.rest[bucket4(&rm).0] += 1;
        if let Ok(rest) = pose::skin_positions(&n, si, &rm, &key, &b) {
            let Some(Data::Mesh(mesh)) = &blk.decoded else {
                continue;
            };
            let mut worst: f64 = 0.;
            for (v, p) in mesh.vertices.as_ref()?.iter().zip(&rest) {
                let s = v.position?.map(f64::from);
                let d = (0..3)
                    .map(|k| (s[k] + bind.translation[k] - p[k]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                worst = worst.max(d);
            }
            let k = [0.001, 0.01, 0.1, 1., 10.]
                .iter()
                .position(|l| worst < *l)
                .unwrap_or(5);
            f.moved[k] += 1;
        }
    }
    Some(())
}

fn rest(
    store: &Store,
    db: &mut Db<'_>,
    vfs: &fo4_vfs::Vfs,
    m: &Manifest,
    out: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    // RACE: the ANAM after each MNAM / FNAM marker.
    let mut sections: HashMap<(String, u32), [Option<String>; 2]> = HashMap::new();
    let mut marker_counts: BTreeMap<String, u64> = BTreeMap::new();
    for k in keys(store, b"RACE") {
        let Some(r) = db.get(&k) else { continue };
        let mut marker = None;
        let mut s: [Option<String>; 2] = [None, None];
        let mut sequence = String::new();
        for (sig, d) in &r.fields {
            match sig {
                b"MNAM" => marker = Some(0),
                b"FNAM" => marker = Some(1),
                b"NAM1" | b"NAM3" => marker = None,
                b"ANAM" => {
                    sequence.push_str(match marker {
                        Some(0) => "M",
                        Some(1) => "F",
                        _ => "-",
                    });
                    if let Some(i) = marker {
                        s[i] = Some(text(d));
                    }
                }
                _ => {}
            }
        }
        bump(&mut marker_counts, sequence);
        sections.insert((k.plugin.to_lowercase(), k.object_id), s);
    }
    writeln!(out, "\n## R1 RACE ANAM sequence by preceding marker (M = after MNAM, F = after FNAM): {marker_counts:?}")?;
    let differ = sections
        .values()
        .filter(|s| {
            s[0].is_some()
                && s[1].is_some()
                && s[0].as_ref().map(|p| p.to_lowercase())
                    != s[1].as_ref().map(|p| p.to_lowercase())
        })
        .count();
    writeln!(
        out,
        "races whose two section skeletons differ: {differ} of {}",
        sections.len()
    )?;
    let mut skeletons: HashMap<String, Option<Skel>> = HashMap::new();
    let mut fits: BTreeMap<(String, usize), Fit> = BTreeMap::new();
    let mut pairs = 0u64;
    for k in keys(store, b"ARMA") {
        let Some(r) = db.get(&k) else { continue };
        let mut races = Vec::new();
        for d in r.all(b"RNAM").chain(r.all(b"MODL")) {
            if let Some((rk, rr)) = u32_at(d, 0).and_then(|id| db.reference(&r, id)) {
                if rr.signature == *b"RACE" {
                    races.push(rk);
                }
            }
        }
        for slot in [b"MOD2", b"MOD3"] {
            let Some(model) = r.one(slot).map(|d| text(d)) else {
                continue;
            };
            for race in &races {
                let Some(sec) = sections
                    .get(&(race.plugin.to_lowercase(), race.object_id))
                    .cloned()
                else {
                    continue;
                };
                let differing = sec[0].as_ref().map(|s| s.to_lowercase())
                    != sec[1].as_ref().map(|s| s.to_lowercase());
                for (si, sp) in sec.iter().enumerate() {
                    let Some(sp) = sp else { continue };
                    let skel = skeletons
                        .entry(sp.to_lowercase())
                        .or_insert_with(|| skeleton(vfs, m, sp));
                    let Some(skel) = skel else { continue };
                    pairs += 1;
                    let slot_name = String::from_utf8_lossy(slot).into_owned();
                    fit(
                        vfs,
                        m,
                        &model,
                        skel,
                        fits.entry((slot_name.clone(), si)).or_default(),
                    );
                    if differing {
                        fit(
                            vfs,
                            m,
                            &model,
                            skel,
                            fits.entry((format!("{slot_name} (races whose sections differ)"), si))
                                .or_default(),
                        );
                    }
                }
            }
        }
    }
    writeln!(
        out,
        "\n## R2 ARMA model slot x race section skeleton ({pairs} model/skeleton pairs; {} skeletons read)",
        skeletons.values().filter(|s| s.is_some()).count()
    )?;
    writeln!(out, "Composite = bone node world transform after the stored skin-to-bone transform, per skinned shape with every bone named in the skeleton: [one translation shared within 0.01, within 0.1, translations spread, rotation off identity by more than 0.001]")?;
    for ((slot, si), f) in &fits {
        let mut v: Vec<_> = f.bind_translations.iter().collect();
        v.sort_by_key(|a| std::cmp::Reverse(*a.1));
        writeln!(out, "{slot} x section {}: {} skinned shapes, {} with every bone in the skeleton ({} bone names missing)", si + 1, f.shapes, f.all_bones_named, f.missing_bones)?;
        writeln!(
            out,
            "  bind (the model's own bone nodes) {:?}; rest (the skeleton) {:?}",
            f.bind, f.rest
        )?;
        writeln!(
            out,
            "  most common bind translations {:?}",
            &v[..v.len().min(8)]
        )?;
        writeln!(out, "  model bone node vs skeleton world translation [<0.01, <1, >=1]: measured reading {:?}, static renderer's reading {:?}", f.nodes_measured, f.nodes_static)?;
        writeln!(out, "  rest moves a vertex from its bind position by at most [<0.001, <0.01, <0.1, <1, <10, >=10]: {:?}", f.moved)?;
    }
    Ok(())
}
fn chain(
    store: &Store,
    db: &mut Db<'_>,
    out: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    // C1: ACHR DATA as six floats; position against the parent CELL's XCLC grid.
    let mut in_grid = [0u64; 3];
    let mut angles = [0u64; 2];
    let mut xscl: BTreeMap<String, u64> = BTreeMap::new();
    for (o, cell) in store.placement_cells() {
        if o.signature != *b"ACHR" || o.deleted {
            continue;
        }
        let key = store.overrides().form_key(o.key, store.order());
        let Some(r) = db.get(&key) else { continue };
        let Some(d) = r.one(b"DATA") else { continue };
        let v: Vec<f32> = (0..6).filter_map(|i| f32_at(d, 4 * i)).collect();
        if v.len() != 6 {
            continue;
        }
        for a in &v[3..] {
            angles[(a.abs() <= 2. * std::f32::consts::PI + 1e-4) as usize] += 1;
        }
        if let Some(x) = r.one(b"XSCL").and_then(|d| f32_at(d, 0)) {
            bump(&mut xscl, format!("{x:.2}"));
        }
        let Some(c) = db.get(&cell) else { continue };
        let Some(g) = c.one(b"XCLC") else {
            in_grid[2] += 1;
            continue;
        };
        let (gi, gj) = (u32_at(g, 0).unwrap() as i32, u32_at(g, 4).unwrap() as i32);
        let inside = (v[0] / 4096.).floor() as i32 == gi && (v[1] / 4096.).floor() as i32 == gj;
        in_grid[inside as usize] += 1;
    }
    writeln!(out, "\n## C1 ACHR DATA: exterior position in the cell's XCLC grid square (4096 units): no {} yes {}; interior or no grid {}", in_grid[0], in_grid[1], in_grid[2])?;
    writeln!(
        out,
        "angles within [-2pi, 2pi]: no {} yes {}; XSCL values {xscl:?}",
        angles[0], angles[1]
    )?;

    // C2: ARMO INDX/MODL pairs, ARMA race match for the ARMO's own race, BOD2 words.
    let mut pairing: BTreeMap<String, u64> = BTreeMap::new();
    let mut matches: BTreeMap<usize, u64> = BTreeMap::new();
    let mut matches_by_index: BTreeMap<String, u64> = BTreeMap::new();
    let mut subset = [0u64; 2];
    let mut bits: BTreeMap<u32, u64> = BTreeMap::new();
    for k in keys(store, b"ARMO") {
        let Some(r) = db.get(&k) else { continue };
        let seq: String = r
            .fields
            .iter()
            .filter(|f| matches!(&f.0, b"INDX" | b"MODL"))
            .map(|f| if &f.0 == b"INDX" { 'I' } else { 'M' })
            .collect();
        let ok = seq.len().is_multiple_of(2) && seq.as_bytes().chunks(2).all(|c| c == b"IM");
        bump(
            &mut pairing,
            if ok {
                "INDX,MODL pairs".to_string()
            } else if seq.is_empty() {
                "none".into()
            } else {
                "other".into()
            },
        );
        let armo_bod2 = r.one(b"BOD2").and_then(|d| u32_at(d, 0)).unwrap_or(0);
        for bit in 0..32 {
            if armo_bod2 & (1 << bit) != 0 {
                bump(&mut bits, bit);
            }
        }
        let Some(race) = r
            .one(b"RNAM")
            .and_then(|d| u32_at(d, 0))
            .and_then(|id| db.reference(&r, id))
            .map(|x| x.0)
        else {
            continue;
        };
        let mut n = 0;
        let mut per_index: BTreeMap<u16, usize> = BTreeMap::new();
        let mut index = 0u16;
        for (sig, d) in &r.fields {
            if sig == b"INDX" {
                index = u16::from_le_bytes([d[0], d[1]]);
            }
            if sig != b"MODL" {
                continue;
            }
            let Some((_, a)) = u32_at(d, 0).and_then(|id| db.reference(&r, id)) else {
                continue;
            };
            let arma_bod2 = a.one(b"BOD2").and_then(|d| u32_at(d, 0)).unwrap_or(0);
            subset[(arma_bod2 & !armo_bod2 == 0) as usize] += 1;
            let mut races = Vec::new();
            for d in a.all(b"RNAM").chain(a.all(b"MODL")) {
                if let Some((rk, _)) = u32_at(d, 0).and_then(|id| db.reference(&a, id)) {
                    races.push(rk);
                }
            }
            if races.contains(&race) {
                n += 1;
                *per_index.entry(index).or_default() += 1;
            }
        }
        bump(&mut matches, n);
        if n > 1 {
            bump(
                &mut matches_by_index,
                if per_index.values().all(|v| *v == 1) {
                    "one per INDX value".into()
                } else {
                    "several with one INDX value".into()
                },
            );
        }
    }
    writeln!(out, "\n## C2 ARMO INDX/MODL sequence {pairing:?}")?;
    writeln!(out, "ARMA (via MODL) whose RNAM or MODL races include the ARMO's RNAM race, per ARMO: {matches:?}; when several: {matches_by_index:?}")?;
    writeln!(
        out,
        "ARMA BOD2 bits within the ARMO BOD2 bits: no {} yes {}; ARMO BOD2 bit use {bits:?}",
        subset[0], subset[1]
    )?;

    // C4: what distinguishes the two model slots and the two race sections.
    fn diff(a: &str, b: &str) -> (String, String) {
        let (a, b) = (a.to_lowercase(), b.to_lowercase());
        let p = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
        let s = a[p..]
            .bytes()
            .rev()
            .zip(b[p..].bytes().rev())
            .take_while(|(x, y)| x == y)
            .count();
        (a[p..a.len() - s].to_string(), b[p..b.len() - s].to_string())
    }
    let mut slot_diffs: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut same = [0u64; 3];
    for k in keys(store, b"ARMA") {
        let Some(r) = db.get(&k) else { continue };
        match (
            r.one(b"MOD2").map(|d| text(d)),
            r.one(b"MOD3").map(|d| text(d)),
        ) {
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(&b) => same[0] += 1,
            (Some(a), Some(b)) => {
                same[1] += 1;
                bump(&mut slot_diffs, diff(&a, &b));
            }
            _ => same[2] += 1,
        }
    }
    let f_token = slot_diffs
        .iter()
        .filter(|((a, b), _)| b.contains('f') && !a.contains('f'))
        .map(|(_, n)| n)
        .sum::<u64>();
    let f_reverse = slot_diffs
        .iter()
        .filter(|((a, b), _)| a.contains('f') && !b.contains('f'))
        .map(|(_, n)| n)
        .sum::<u64>();
    writeln!(out, "\n## C4 ARMA with differing MOD2/MOD3 whose MOD3 differing part has an f and MOD2's none: {f_token}; the reverse: {f_reverse}")?;
    let mut v: Vec<_> = slot_diffs.into_iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    writeln!(out, "ARMA MOD2/MOD3: equal {} differ {} one or both absent {}; differing part (MOD2, MOD3), top: {:?}", same[0], same[1], same[2], &v[..v.len().min(15)])?;
    let mut sec_diffs: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut voice_diffs: BTreeMap<(String, String), u64> = BTreeMap::new();
    for k in keys(store, b"RACE") {
        let Some(r) = db.get(&k) else { continue };
        let mut marker = None;
        let mut group = "";
        let mut seen: BTreeMap<(String, usize), String> = BTreeMap::new();
        for (sig, d) in &r.fields {
            match sig {
                b"MNAM" => marker = Some(0),
                b"FNAM" => marker = Some(1),
                b"NAM1" => {
                    group = "NAM1";
                    marker = None
                }
                b"NAM3" => {
                    group = "NAM3";
                    marker = None
                }
                b"ANAM" | b"MODL" => {
                    if let Some(i) = marker {
                        seen.entry((format!("{group}.{}", String::from_utf8_lossy(sig)), i))
                            .or_insert_with(|| text(d));
                    }
                }
                _ => {}
            }
        }
        let groups: std::collections::BTreeSet<String> = seen.keys().map(|k| k.0.clone()).collect();
        for g in groups {
            if let (Some(a), Some(b)) = (seen.get(&(g.clone(), 0)), seen.get(&(g.clone(), 1))) {
                if !a.eq_ignore_ascii_case(b) {
                    let (x, y) = diff(a, b);
                    bump(&mut sec_diffs, (format!("{g}:{x}"), y));
                }
            }
        }
        if let Some(vt) = r.one(b"VTCK").cloned() {
            let names: Vec<String> = [0, 4]
                .iter()
                .filter_map(|o| u32_at(&vt, *o))
                .filter_map(|id| db.reference(&r, id))
                .map(|(_, v)| v.one(b"EDID").map(|d| text(d)).unwrap_or_default())
                .collect();
            if names.len() == 2 && names[0] != names[1] {
                bump(&mut voice_diffs, diff(&names[0], &names[1]));
            }
        }
    }
    let mut v: Vec<_> = sec_diffs.into_iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    writeln!(
        out,
        "RACE: differing part of the MNAM-section and FNAM-section path by group, top: {:?}",
        &v[..v.len().min(15)]
    )?;
    let mut v: Vec<_> = voice_diffs.into_iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    writeln!(
        out,
        "RACE VTCK: differing part of the two VTYP editor IDs (slot 0, slot 1), top: {:?}",
        &v[..v.len().min(10)]
    )?;

    // C3: OTFT INAM targets and LVLO layout (u16 level at 0, FormID at 4, u16 at 8).
    let mut targets: BTreeMap<String, u64> = BTreeMap::new();
    for k in keys(store, b"OTFT") {
        let Some(r) = db.get(&k) else { continue };
        for d in r.all(b"INAM") {
            for o in (0..d.len()).step_by(4) {
                let t = u32_at(d, o)
                    .and_then(|id| db.reference(&r, id))
                    .map(|x| String::from_utf8_lossy(&x.1.signature).into_owned());
                bump(&mut targets, t.unwrap_or_else(|| "unresolved".into()));
            }
        }
    }
    writeln!(
        out,
        "\n## C3 OTFT INAM four-byte entries resolve to {targets:?}"
    )?;
    for sig in [b"LVLN", b"LVLI"] {
        let mut llct = [0u64; 2];
        let mut words: BTreeMap<String, u64> = BTreeMap::new();
        let mut lvld: BTreeMap<u8, u64> = BTreeMap::new();
        for k in keys(store, sig) {
            let Some(r) = db.get(&k) else { continue };
            let n = r.all(b"LVLO").count();
            let declared = r.one(b"LLCT").and_then(|d| d.first().copied()).unwrap_or(0) as usize;
            llct[(n == declared) as usize] += 1;
            if let Some(v) = r.one(b"LVLD").and_then(|d| d.first().copied()) {
                bump(&mut lvld, v);
            }
            for d in r.all(b"LVLO") {
                bump(
                    &mut words,
                    format!(
                        "+2:{:04x} +10:{:04x}",
                        u16::from_le_bytes([d[2], d[3]]),
                        u16::from_le_bytes([d[10], d[11]])
                    ),
                );
            }
        }
        let mut w: Vec<_> = words.into_iter().collect();
        w.sort_by_key(|a| std::cmp::Reverse(a.1));
        writeln!(out, "{}: LLCT equals the LVLO count: no {} yes {}; LVLD bytes {lvld:?}; LVLO u16 words at +2/+10 (top) {:?}", String::from_utf8_lossy(sig), llct[0], llct[1], &w[..w.len().min(6)])?;
    }
    Ok(())
}
/// Every placed actor of the load order resolved as the viewer does.
fn placed(
    store: &Store,
    vfs: &fo4_vfs::Vfs,
    m: &Manifest,
    out: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let budget = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let mut loader = fo4_scene::Loader::new(
        store,
        vfs,
        &m.language,
        fo4_scene::RuleLevel::Hypotheses,
        &budget.account(),
    );
    let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
    let mut notes: BTreeMap<String, u64> = BTreeMap::new();
    let mut groups: BTreeMap<String, u64> = BTreeMap::new();
    let mut n = 0u64;
    let mut multi: BTreeMap<String, u64> = BTreeMap::new();
    let cells: Vec<FormKey> = store.cells().collect();
    for cell in &cells {
        let Ok(children) = store.children(cell, &budget.child()) else {
            bump(&mut groups, "cell children unreadable".into());
            continue;
        };
        for (list, kind) in [
            (&children.excluded, "placed"),
            (&children.disabled, "initially disabled"),
            (&children.deleted, "deleted"),
        ] {
            for c in list.iter().filter(|c| c.signature == *b"ACHR") {
                let kind = if c.group_kind == 10 && kind == "placed" {
                    "distant group"
                } else {
                    kind
                };
                bump(&mut groups, kind.into());
                if kind != "placed" {
                    continue;
                }
                n += 1;
                if n.is_multiple_of(400) {
                    loader.reset_asset_cache(&budget.account());
                }
                let scope = budget.child();
                let a = match fo4_scene::actor::resolve_reference(
                    &mut loader,
                    &c.key,
                    &c.source,
                    &scope,
                ) {
                    Ok(a) => a,
                    Err(e) => {
                        bump(&mut outcomes, format!("error {}", e.code));
                        loader.reset_asset_cache(&budget.account());
                        continue;
                    }
                };
                let outcome = match (&a.plan, a.state.ready_at(fo4_scene::RuleLevel::Hypotheses)) {
                    (Some(_), true) => "resolved, drawable under hypotheses".to_string(),
                    (Some(_), false) => {
                        let codes: std::collections::BTreeSet<String> = a
                            .blockers
                            .iter()
                            .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
                            .map(|e| e.code.clone())
                            .collect();
                        format!("resolved, blocked by {codes:?}")
                    }
                    (None, _) => {
                        let first = a
                            .notes
                            .iter()
                            .find(|n| {
                                !n.starts_with("head:")
                                    && !n.starts_with("outfit: none")
                                    && !n.starts_with("outfit: leveled item list")
                                    && !n.starts_with("skin addon")
                            })
                            .cloned()
                            .unwrap_or_default();
                        // Keep the reason, not the record identities in it.
                        let reason: String = first
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .split_whitespace()
                            .filter(|w| {
                                !w.contains(".esm:")
                                    && !w.contains(".esp:")
                                    && !w.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        format!("not resolved: {reason}")
                    }
                };
                if a.plan.as_ref().is_some_and(|p| p.parts.len() >= 2)
                    && a.state.ready_at(fo4_scene::RuleLevel::Hypotheses)
                {
                    bump(
                        &mut multi,
                        format!("{}:{:06x}", cell.plugin, cell.object_id),
                    );
                }
                bump(&mut outcomes, outcome);
                for note in &a.notes {
                    if note.starts_with("head:") {
                        bump(&mut notes, "head: not read".into());
                    } else if note.starts_with("skin addon") {
                        bump(&mut notes, "skin addon hidden by worn armour slots".into());
                    } else if note.starts_with("outfit: none") {
                        bump(&mut notes, "outfit: none declared".into());
                    } else if note.starts_with("armour") && note.contains("no addon for race") {
                        bump(&mut notes, "armour: no addon for race".into());
                    }
                }
            }
        }
    }
    writeln!(
        out,
        "\n## A1 placed ACHR of every cell child group: {groups:?}"
    )?;
    writeln!(
        out,
        "resolution of the {n} placed (not disabled, not distant) actors:"
    )?;
    let mut v: Vec<_> = outcomes.into_iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (k, c) in v {
        writeln!(out, "  {c}\t{k}")?;
    }
    writeln!(
        out,
        "notes on actors (an actor may have several): {notes:?}"
    )?;
    let mut v: Vec<_> = multi.into_iter().collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.1));
    writeln!(
        out,
        "cells with the most drawable actors of two or more parts: {:?}",
        &v[..v.len().min(10)]
    )?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
