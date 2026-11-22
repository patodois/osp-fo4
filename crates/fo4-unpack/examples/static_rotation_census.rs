#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop, clippy::type_complexity)]
//! Census of nested node rotations in models drawn by the static renderer,
//! and internal checks of the two readings of a node's nine rotation words.
//!
//! The static renderer's previous reading mapped a child point p of a node to
//! `t + s * R^T * p`, R being the nine stored words as three rows (called
//! "cols" in the report: the words as columns); the actor step
//! (`fo4_scene::actor::pose::Affine`) and now `fo4_scene::transform::Transform`
//! map it to `t + s * R * p` ("rows"). For every winning NIF this walks the nodes and shapes the
//! static renderer walks, composes both readings, and counts the shapes whose
//! vertices land elsewhere. The checks that need no running game compare the
//! two model boxes with boxes stored in the data: the OBND of every base
//! record that names the model, the `BSBound` extra data of the model and the
//! `BSMultiBoundAABB` of a `BSMultiBoundNode`. Measurement support for
//! `spec/scene.md`; reads one file at a time and writes a plain-text report,
//! and optionally a per-model table (real paths, kept outside the repository).
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Data, Nif, Value};
use fo4_plugin::typed::Store;
use fo4_scene::{actor::pose::Affine, model_policy, transform::Transform};
use fo4_vfs::Vfs;
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

fn bump<K: Ord>(m: &mut BTreeMap<K, u64>, k: K) {
    *m.entry(k).or_default() += 1;
}
fn link(fields: &[fo4_nif::Field], name: &str) -> Vec<i32> {
    fields
        .iter()
        .filter(|f| f.name == name)
        .filter_map(|f| match f.value {
            Value::Ref(r) if r >= 0 => Some(r),
            _ => None,
        })
        .collect()
}
fn floats(fields: &[fo4_nif::Field], name: &str) -> Vec<f64> {
    fields
        .iter()
        .filter(|f| f.name == name)
        .filter_map(|f| match f.value {
            Value::FloatBits(b) => Some(f64::from(f32::from_bits(b))),
            _ => None,
        })
        .collect()
}
fn f32_at(d: &[u8], o: usize) -> Option<f64> {
    d.get(o..o + 4)
        .map(|v| f64::from(f32::from_le_bytes(v.try_into().unwrap())))
}

#[derive(Clone, Copy)]
struct Box3 {
    min: [f64; 3],
    max: [f64; 3],
}
impl Box3 {
    /// Inside `min..max` widened by `tol`.
    fn inside(&self, min: [f64; 3], max: [f64; 3], tol: f64) -> bool {
        (0..3).all(|i| self.min[i] >= min[i] - tol && self.max[i] <= max[i] + tol)
    }
    fn add(this: &mut Option<Self>, p: [f64; 3]) {
        match this {
            None => {
                *this = Some(Self { min: p, max: p });
            }
            Some(b) => {
                for i in 0..3 {
                    b.min[i] = b.min[i].min(p[i]);
                    b.max[i] = b.max[i].max(p[i]);
                }
            }
        }
    }
    fn join(this: &mut Option<Self>, o: Option<Self>) {
        if let Some(o) = o {
            Self::add(this, o.min);
            Self::add(this, o.max);
        }
    }
    /// Largest coordinate difference of the six box words.
    fn err(&self, min: [f64; 3], max: [f64; 3]) -> f64 {
        (0..3)
            .map(|i| {
                (self.min[i] - min[i])
                    .abs()
                    .max((self.max[i] - max[i]).abs())
            })
            .fold(0., f64::max)
    }
    /// Largest difference of centre and half extents.
    fn err_centre(&self, c: [f64; 3], h: [f64; 3]) -> f64 {
        (0..3)
            .map(|i| {
                let bc = (self.min[i] + self.max[i]) / 2.;
                let bh = (self.max[i] - self.min[i]) / 2.;
                (bc - c[i]).abs().max((bh - h[i]).abs())
            })
            .fold(0., f64::max)
    }
}

/// The static renderer's reading before this census: the nine words as the
/// columns of the matrix acting on column vectors (`Transform::from_fields`
/// now reads them as rows, the column reading below).
fn previous(f: &[fo4_nif::Field]) -> fo4_core::Result<Transform> {
    let t = Transform::from_fields(f, "census")?;
    Ok(Transform {
        rotation: std::array::from_fn(|i| std::array::from_fn(|j| t.rotation[j][i])),
        ..t
    })
}
fn rotated(rows: &[[f64; 3]; 3]) -> bool {
    (0..3).any(|i| (0..3).any(|j| (rows[i][j] - if i == j { 1. } else { 0. }).abs() > 1e-5))
}
fn symmetric(rows: &[[f64; 3]; 3]) -> bool {
    (0..3).all(|i| (0..3).all(|j| (rows[i][j] - rows[j][i]).abs() <= 1e-5))
}

#[derive(Default)]
struct ModelResult {
    shapes: u64,
    skinned: u64,
    /// Shapes whose largest vertex displacement between readings is above
    /// 0.01, and the largest displacement.
    moved: u64,
    largest: f64,
    /// Shapes by number of non-identity rotations on their chain (shape
    /// included), and with a non-symmetric one.
    by_chain: BTreeMap<u64, u64>,
    nested: u64,
    asymmetric: u64,
    boxes: [Option<Box3>; 2],
    /// Box of the shapes that move, under each reading.
    moved_boxes: [Option<Box3>; 2],
    bsbound: Option<([f64; 3], [f64; 3])>,
    /// Per BSMultiBoundAABB: stored position, extent, and the subtree box
    /// under each reading.
    multibound: Vec<([f64; 3], [f64; 3], [Option<Box3>; 2])>,
    multibound_sizes: BTreeMap<usize, u64>,
    /// Editor-marker subtrees left out, as the static renderer does.
    markers: u64,
    /// A root node with a non-identity rotation.
    root_rotated: bool,
}

struct Walker<'a, 'n> {
    n: &'a Nif<'n>,
    /// Leave the root nodes' own transforms out (comparison only).
    skip_root: bool,
    out: ModelResult,
    visits: u64,
}
impl Walker<'_, '_> {
    /// Returns the subtree's box under each reading.
    fn visit(
        &mut self,
        i: usize,
        s: Transform,
        c: Affine,
        rot: u64,
        asym: bool,
        depth: u32,
    ) -> [Option<Box3>; 2] {
        let mut boxes = [None, None];
        self.visits += 1;
        if depth > 64 || self.visits > 1_000_000 {
            return boxes;
        }
        let Some(block) = self.n.blocks.get(i) else {
            return boxes;
        };
        let ty = &self.n.block_types[block.type_index as usize];
        if model_policy::visibility_editor_marker(self.n, i)
            || model_policy::editor_marker_object(self.n, i)
            || model_policy::editor_marker_suffix(self.n, i)
        {
            self.out.markers += 1;
            return boxes;
        }
        if matches!(
            model_policy::block_class(ty),
            fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
        ) {
            return boxes;
        }
        let Some(data) = block.decoded.as_ref() else {
            return boxes;
        };
        let f = model_policy::fields(data);
        let (s, c, rot, asym) =
            if f.iter().any(|f| f.name == "Translation") && !(depth == 0 && self.skip_root) {
                let Ok(ls) = previous(f) else {
                    return boxes;
                };
                let Ok(Some(lc)) = Affine::from_fields(f, "census") else {
                    return boxes;
                };
                let r = rotated(&lc.rows);
                if depth == 0 && r {
                    self.out.root_rotated = true;
                }
                (
                    s.compose(ls),
                    c.compose(&lc),
                    rot + u64::from(r),
                    asym || r && !symmetric(&lc.rows),
                )
            } else {
                (s, c, rot, asym)
            };
        match data {
            Data::Node { children, fields } => {
                for ch in children.iter().filter(|c| **c >= 0) {
                    let b = self.visit(*ch as usize, s, c, rot, asym, depth + 1);
                    Box3::join(&mut boxes[0], b[0]);
                    Box3::join(&mut boxes[1], b[1]);
                }
                if ty == "BSMultiBoundNode" {
                    for mb in link(fields, "Multi Bound") {
                        self.multibound(mb as usize, boxes);
                    }
                }
                if depth == 0 {
                    for e in link(fields, "Extra Data") {
                        let Some(Data::Fields(ef)) = self
                            .n
                            .blocks
                            .get(e as usize)
                            .and_then(|b| b.decoded.as_ref())
                        else {
                            continue;
                        };
                        let centre = floats(ef, "Center");
                        let dims = floats(ef, "Dimensions");
                        if centre.len() == 3 && dims.len() == 3 {
                            self.out.bsbound = Some((
                                [centre[0], centre[1], centre[2]],
                                [dims[0], dims[1], dims[2]],
                            ));
                        }
                    }
                }
            }
            _ => {
                let mesh = match data {
                    Data::Mesh(m) => Some(m),
                    Data::Fields(ff) => link(ff, "Data")
                        .first()
                        .and_then(|d| self.n.blocks.get(*d as usize))
                        .and_then(|b| match b.decoded.as_ref() {
                            Some(Data::Mesh(m)) => Some(m),
                            _ => None,
                        }),
                    _ => None,
                };
                let Some(mesh) = mesh else {
                    return boxes;
                };
                let skinned = !link(f, "Skin").is_empty() || !link(f, "Skin Instance").is_empty();
                self.out.shapes += 1;
                if skinned {
                    self.out.skinned += 1;
                    return boxes;
                }
                bump(&mut self.out.by_chain, rot.min(4));
                if rot >= 2 {
                    self.out.nested += 1;
                }
                if asym {
                    self.out.asymmetric += 1;
                }
                let mut largest: f64 = 0.;
                let mut own = [None, None];
                for v in mesh.vertices.iter().flatten() {
                    let Some(p) = v.position else { continue };
                    let p = p.map(f64::from);
                    let a = s.apply(p);
                    let b = c.apply(p);
                    let d = (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f64>().sqrt();
                    largest = largest.max(d);
                    Box3::add(&mut own[0], a);
                    Box3::add(&mut own[1], b);
                }
                for k in 0..2 {
                    Box3::join(&mut boxes[k], own[k]);
                }
                if largest > 0.01 {
                    self.out.moved += 1;
                    for k in 0..2 {
                        Box3::join(&mut self.out.moved_boxes[k], own[k]);
                    }
                }
                self.out.largest = self.out.largest.max(largest);
            }
        }
        boxes
    }
    fn multibound(&mut self, i: usize, boxes: [Option<Box3>; 2]) {
        let n = self.n;
        let Some(b) = n.blocks.get(i) else { return };
        if n.block_types[b.type_index as usize] != "BSMultiBound" {
            return;
        }
        let raw = &n.bytes[b.data.clone()];
        bump(&mut self.out.multibound_sizes, raw.len() + 1000);
        let Some(d) = raw
            .get(0..4)
            .map(|v| i32::from_le_bytes(v.try_into().unwrap()))
        else {
            return;
        };
        let Some(db) = (d >= 0).then(|| n.blocks.get(d as usize)).flatten() else {
            return;
        };
        if n.block_types[db.type_index as usize] != "BSMultiBoundAABB" {
            return;
        }
        let raw = &n.bytes[db.data.clone()];
        bump(&mut self.out.multibound_sizes, raw.len());
        if raw.len() != 24 {
            return;
        }
        let v: Vec<f64> = (0..6).filter_map(|k| f32_at(raw, 4 * k)).collect();
        self.out
            .multibound
            .push(([v[0], v[1], v[2]], [v[3], v[4], v[5]], boxes));
    }
}

#[derive(Default)]
struct Compare {
    /// Static reading closer, column reading closer, tie (within 0.01).
    closer: [u64; 3],
    /// Within the tolerance: cols only, rows only, both, neither.
    within: [u64; 4],
    /// Error histograms per reading.
    hist: [BTreeMap<&'static str, u64>; 2],
}
fn bucket(e: f64) -> &'static str {
    match e {
        e if e < 0.01 => "a <0.01",
        e if e < 0.1 => "b <0.1",
        e if e < 1.01 => "c <1.01",
        e if e < 2.01 => "d <2.01",
        e if e < 10. => "e <10",
        e if e < 100. => "f <100",
        _ => "g >=100",
    }
}
impl Compare {
    fn add(&mut self, es: f64, ec: f64, tol: f64) {
        let k = if (es - ec).abs() <= 0.01 {
            2
        } else if es < ec {
            0
        } else {
            1
        };
        self.closer[k] += 1;
        let w = match (es <= tol, ec <= tol) {
            (true, false) => 0,
            (false, true) => 1,
            (true, true) => 2,
            (false, false) => 3,
        };
        self.within[w] += 1;
        bump(&mut self.hist[0], bucket(es));
        bump(&mut self.hist[1], bucket(ec));
    }
    fn write(&self, out: &mut String, name: &str, tol: f64) -> std::fmt::Result {
        writeln!(
            out,
            "{name}\tcloser: cols {}, rows {}, tie {}",
            self.closer[0], self.closer[1], self.closer[2]
        )?;
        writeln!(
            out,
            "{name}\twithin {tol}: cols only {}, rows only {}, both {}, neither {}",
            self.within[0], self.within[1], self.within[2], self.within[3]
        )?;
        for (r, h) in ["cols", "rows"].iter().zip(&self.hist) {
            let v: Vec<String> = h.iter().map(|(k, n)| format!("{k}:{n}")).collect();
            writeln!(out, "{name}\t{r} error {}", v.join(" "))?;
        }
        Ok(())
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("static_rotation_census MANIFEST OUT.txt [MODELS.tsv]".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    if args[1] == "--dump" {
        return dump(&m, &args[2..]);
    }
    // Base records with a model path and an OBND.
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut obnd: BTreeMap<Vec<u8>, Vec<([i16; 6], [u8; 4])>> = BTreeMap::new();
    let mut records_with_model = BTreeMap::<String, u64>::new();
    {
        let keys: Vec<_> = store
            .winners()
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        for key in keys {
            let b = Budget::new(m.limits.max_memory);
            let Some(r) = store.raw(&key, &b)? else {
                continue;
            };
            if r.deleted() {
                continue;
            }
            let mut model = None;
            let mut bounds = None;
            for s in &r.subrecords {
                let d = r.data(s);
                if s.signature == *b"MODL" && model.is_none() && d.len() > 4 {
                    model = Some(d.strip_suffix(&[0]).unwrap_or(d).to_vec());
                }
                if s.signature == *b"OBND" && d.len() == 12 {
                    bounds = Some(std::array::from_fn(|k| {
                        i16::from_le_bytes([d[2 * k], d[2 * k + 1]])
                    }));
                }
            }
            let (Some(model), Some(bounds)) = (model, bounds) else {
                continue;
            };
            let Ok(path) =
                fo4_scene::paths::resolve(fo4_scene::paths::Citation::RecordModel, &model, &b)
            else {
                continue;
            };
            bump(
                &mut records_with_model,
                String::from_utf8_lossy(&r.signature).into_owned(),
            );
            obnd.entry(path).or_default().push((bounds, r.signature));
        }
    }
    drop(store);
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut paths: Vec<Vec<u8>> = vfs
        .candidates
        .iter()
        .filter_map(|c| c.key.clone())
        .filter(|k| k.ends_with(b".nif"))
        .collect();
    paths.sort();
    paths.dedup();
    let mut out = String::new();
    let mut table = String::from("path\tbase_records\tshapes\tmoved\tlargest\tobnd_cols\tobnd_rows\tbsbound_cols\tbsbound_rows\tmoved_inside_obnd\n");
    let (mut files, mut failed, mut named) = (0u64, 0u64, 0u64);
    let mut shapes = 0u64;
    let mut markers = 0u64;
    let mut skinned = 0u64;
    let mut moved_shapes = 0u64;
    let mut nested = 0u64;
    let mut asym = 0u64;
    let mut by_chain = BTreeMap::<u64, u64>::new();
    let mut moved_models = [0u64; 2];
    let mut moved_named = [0u64; 2];
    let mut largest_hist = BTreeMap::<&'static str, u64>::new();
    let mut mb_sizes = BTreeMap::<usize, u64>::new();
    let (mut c_obnd, mut c_obnd_all, mut c_bsb, mut c_mb) = (
        Compare::default(),
        Compare::default(),
        Compare::default(),
        Compare::default(),
    );
    let (mut obnd_static_all, mut obnd_n_all) = (0u64, 0u64);
    // Moved shapes inside OBND (widened by 1.01): cols only, rows only,
    // both, neither; per base record of a model moving more than 1 unit.
    let mut inside = [0u64; 4];
    let mut inside_models = [0u64; 4];
    let mut bsbound_files = 0u64;
    let (mut root_rotated, mut root_rotated_named) = (0u64, 0u64);
    // OBND within 1.01 of the model box by variant bit: 1 static, 2 column,
    // 4 static without the root transform, 8 column without it; over base
    // records whose model's four boxes differ by more than 1.
    let mut variants = [0u64; 16];
    for path in &paths {
        let b = Budget::new(m.limits.max_memory);
        let Ok(bytes) = vfs.read_path_bytes(path, &b) else {
            failed += 1;
            continue;
        };
        let name = String::from_utf8_lossy(path).into_owned();
        let Ok(n) = Nif::read(&bytes, &name, Game::Fallout4, &m.limits, &b) else {
            failed += 1;
            continue;
        };
        files += 1;
        let walk = |skip_root| {
            let mut w = Walker {
                n: &n,
                skip_root,
                out: ModelResult::default(),
                visits: 0,
            };
            let mut boxes = [None, None];
            for r in n.roots.iter().filter(|r| **r >= 0) {
                let bx = w.visit(
                    *r as usize,
                    Transform::IDENTITY,
                    Affine::IDENTITY,
                    0,
                    false,
                    0,
                );
                Box3::join(&mut boxes[0], bx[0]);
                Box3::join(&mut boxes[1], bx[1]);
            }
            ModelResult { boxes, ..w.out }
        };
        let r = walk(false);
        let r_skip = walk(true);
        if r.root_rotated {
            root_rotated += 1;
            if obnd.contains_key(path) {
                root_rotated_named += 1;
            }
        }
        if let (Some(recs), [Some(a), Some(b)], [Some(c), Some(d)]) =
            (obnd.get(path), r.boxes, r_skip.boxes)
        {
            let v = [a, b, c, d];
            let differ = v.iter().any(|x| {
                v.iter().any(|y| {
                    (0..3).any(|i| {
                        (x.min[i] - y.min[i]).abs() > 1. || (x.max[i] - y.max[i]).abs() > 1.
                    })
                })
            });
            if differ {
                for (o, _) in recs {
                    let min = [o[0], o[1], o[2]].map(f64::from);
                    let max = [o[3], o[4], o[5]].map(f64::from);
                    let mask = v
                        .iter()
                        .enumerate()
                        .filter(|(_, x)| x.err(min, max) <= 1.01)
                        .fold(0usize, |m, (k, _)| m | 1 << k);
                    variants[mask] += 1;
                }
            }
        }
        shapes += r.shapes;
        markers += r.markers;
        skinned += r.skinned;
        moved_shapes += r.moved;
        nested += r.nested;
        asym += r.asymmetric;
        for (k, v) in &r.by_chain {
            *by_chain.entry(*k).or_default() += v;
        }
        for (k, v) in &r.multibound_sizes {
            *mb_sizes.entry(*k).or_default() += v;
        }
        let recs = obnd.get(path);
        if recs.is_some() {
            named += 1;
        }
        if r.moved > 0 {
            moved_models[0] += 1;
            bump(&mut largest_hist, bucket(r.largest));
            if recs.is_some() {
                moved_named[0] += 1;
            }
        }
        if r.largest > 1. {
            moved_models[1] += 1;
            if recs.is_some() {
                moved_named[1] += 1;
            }
        }
        let (mut os, mut oc) = (f64::NAN, f64::NAN);
        let mut inside_tag = "-";
        if let (Some(recs), [Some(bs), Some(bc)]) = (recs, r.boxes) {
            for (o, _) in recs {
                let min = [o[0], o[1], o[2]].map(f64::from);
                let max = [o[3], o[4], o[5]].map(f64::from);
                let es = bs.err(min, max);
                let ec = bc.err(min, max);
                obnd_n_all += 1;
                if es <= 1.01 {
                    obnd_static_all += 1;
                }
                c_obnd_all.add(es, ec, 1.01);
                if r.largest > 1. {
                    c_obnd.add(es, ec, 1.01);
                }
                os = es;
                oc = ec;
            }
            if let ([Some(ms), Some(mc)], true) = (r.moved_boxes, r.largest > 1.) {
                let mut any = [false; 4];
                for (o, _) in recs {
                    let min = [o[0], o[1], o[2]].map(f64::from);
                    let max = [o[3], o[4], o[5]].map(f64::from);
                    let k = match (ms.inside(min, max, 1.01), mc.inside(min, max, 1.01)) {
                        (true, false) => 0,
                        (false, true) => 1,
                        (true, true) => 2,
                        (false, false) => 3,
                    };
                    inside[k] += 1;
                    any[k] = true;
                }
                if let Some(k) = any.iter().position(|a| *a) {
                    inside_models[k] += 1;
                    inside_tag = ["cols", "rows", "both", "neither"][k];
                }
            }
        }
        if r.bsbound.is_some() {
            bsbound_files += 1;
        }
        let (mut bss, mut bsc) = (f64::NAN, f64::NAN);
        if let (Some((c, d)), [Some(bs), Some(bc)]) = (r.bsbound, r.boxes) {
            bss = bs.err_centre(c, d);
            bsc = bc.err_centre(c, d);
            if r.largest > 1. {
                c_bsb.add(bss, bsc, 0.01);
            }
        }
        for (p, e, bx) in &r.multibound {
            if let [Some(bs), Some(bc)] = bx {
                let es = bs.err_centre(*p, *e);
                let ec = bc.err_centre(*p, *e);
                if (0..3).any(|i| {
                    (bs.min[i] - bc.min[i]).abs() > 1. || (bs.max[i] - bc.max[i]).abs() > 1.
                }) {
                    c_mb.add(es, ec, 0.01);
                }
            }
        }
        if r.moved > 0 {
            writeln!(
                table,
                "{name}\t{}\t{}\t{}\t{:.3}\t{os:.3}\t{oc:.3}\t{bss:.3}\t{bsc:.3}\t{inside_tag}",
                recs.map_or(0, Vec::len),
                r.shapes - r.skinned,
                r.moved,
                r.largest
            )?;
        }
    }
    writeln!(out, "# static_rotation_census")?;
    writeln!(
        out,
        "# cols: the nine rotation words as the columns of the matrix acting on column vectors (the previous reading); rows: as its rows (the reading adopted)"
    )?;
    writeln!(
        out,
        "files\t{files} read, {failed} not read; {named} named by a base record with MODL and OBND"
    )?;
    let v: Vec<String> = records_with_model
        .iter()
        .map(|(k, n)| format!("{k}:{n}"))
        .collect();
    writeln!(out, "base records with MODL and OBND\t{}", v.join(" "))?;
    writeln!(
        out,
        "shapes\t{shapes} reached ({skinned} skinned, left out below); editor-marker subtrees left out {markers}"
    )?;
    let v: Vec<String> = by_chain.iter().map(|(k, n)| format!("{k}:{n}")).collect();
    writeln!(
        out,
        "shapes by non-identity rotations on the chain (4 = 4 or more)\t{}",
        v.join(" ")
    )?;
    writeln!(
        out,
        "shapes with two or more\t{nested}; with a non-symmetric one\t{asym}"
    )?;
    writeln!(
        out,
        "shapes whose vertices move by more than 0.01 between readings\t{moved_shapes}"
    )?;
    writeln!(
        out,
        "models with such a shape\t{} (named by a base record {}); moving more than 1 unit {} (named {})",
        moved_models[0], moved_named[0], moved_models[1], moved_named[1]
    )?;
    let v: Vec<String> = largest_hist
        .iter()
        .map(|(k, n)| format!("{k}:{n}"))
        .collect();
    writeln!(out, "largest displacement per model\t{}", v.join(" "))?;
    let v: Vec<String> = mb_sizes.iter().map(|(k, n)| format!("{k}:{n}")).collect();
    writeln!(
        out,
        "BSMultiBound / AABB block sizes (1000+ = BSMultiBound)\t{}",
        v.join(" ")
    )?;
    writeln!(
        out,
        "OBND against the model box, every base record: cols within 1.01 in {obnd_static_all} of {obnd_n_all}"
    )?;
    writeln!(
        out,
        "OBND containing the moved shapes (widened by 1.01), models moving > 1, per base record\tcols only {}, rows only {}, both {}, neither {}",
        inside[0], inside[1], inside[2], inside[3]
    )?;
    writeln!(
        out,
        "same, per model (first of: cols only, rows only, both, neither)\t{} {} {} {}",
        inside_models[0], inside_models[1], inside_models[2], inside_models[3]
    )?;
    writeln!(out, "files with a BSBound on a root\t{bsbound_files}")?;
    writeln!(
        out,
        "files with a rotated root node\t{root_rotated} (named by a base record {root_rotated_named})"
    )?;
    let v: Vec<String> = variants
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .map(|(k, n)| {
            let names: Vec<&str> = ["cols", "rows", "cols-noroot", "rows-noroot"]
                .iter()
                .enumerate()
                .filter(|(b, _)| k & 1 << b != 0)
                .map(|(_, s)| *s)
                .collect();
            format!(
                "{}:{n}",
                if names.is_empty() {
                    "none".into()
                } else {
                    names.join("+")
                }
            )
        })
        .collect();
    writeln!(
        out,
        "OBND within 1.01 of the model box, by reading and root handling (base records whose four boxes differ)\t{}",
        v.join(" ")
    )?;
    c_obnd_all.write(&mut out, "OBND (all models)", 1.01)?;
    c_obnd.write(&mut out, "OBND (models moving > 1)", 1.01)?;
    c_bsb.write(&mut out, "BSBound (models moving > 1)", 0.01)?;
    c_mb.write(&mut out, "BSMultiBoundAABB (boxes differing > 1)", 0.01)?;
    std::fs::write(&args[1], out)?;
    if let Some(t) = args.get(2) {
        std::fs::write(t, table)?;
    }
    Ok(())
}
/// Prints the node tree of the given models with both readings' world
/// boxes of each shape (exploration only).
fn dump(m: &Manifest, paths: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let vfs = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    for path in paths {
        let b = Budget::new(m.limits.max_memory);
        let bytes = vfs.read_path_bytes(path.as_bytes(), &b)?;
        let n = Nif::read(&bytes, path, Game::Fallout4, &m.limits, &b)?;
        println!("# {path}");
        fn go(
            n: &Nif<'_>,
            i: usize,
            s: Transform,
            c: Affine,
            depth: usize,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let Some(block) = n.blocks.get(i) else {
                return Ok(());
            };
            let ty = &n.block_types[block.type_index as usize];
            let pad = "  ".repeat(depth);
            let name = fo4_scene::actor::pose::name(n, i)
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .unwrap_or_default();
            let Some(data) = block.decoded.as_ref() else {
                println!("{pad}{i} {ty} (raw {} bytes)", block.data.len());
                return Ok(());
            };
            let f = model_policy::fields(data);
            let (s, c) = if f.iter().any(|f| f.name == "Translation") {
                let ls = previous(f)?;
                let lc = Affine::from_fields(f, "dump")?.unwrap_or(Affine::IDENTITY);
                println!(
                    "{pad}{i} {ty} '{name}' t {:?} s {} rows {:?}",
                    lc.translation.map(|v| (v * 1000.).round() / 1000.),
                    lc.scale,
                    lc.rows.map(|r| r.map(|v| (v * 1000.).round() / 1000.))
                );
                (s.compose(ls), c.compose(&lc))
            } else {
                println!("{pad}{i} {ty} '{name}'");
                (s, c)
            };
            match data {
                Data::Node { children, .. } => {
                    for ch in children.iter().filter(|c| **c >= 0) {
                        go(n, *ch as usize, s, c, depth + 1)?;
                    }
                }
                Data::Mesh(mesh) => {
                    let mut own = [None, None];
                    for v in mesh.vertices.iter().flatten() {
                        let Some(p) = v.position else { continue };
                        let p = p.map(f64::from);
                        Box3::add(&mut own[0], s.apply(p));
                        Box3::add(&mut own[1], c.apply(p));
                    }
                    for (k, bx) in own.iter().enumerate() {
                        if let Some(bx) = bx {
                            println!(
                                "{pad}  {} box {:?} .. {:?}",
                                ["cols", "rows"][k],
                                bx.min.map(|v| v.round()),
                                bx.max.map(|v| v.round())
                            );
                        }
                    }
                }
                Data::Fields(_) => {}
            }
            Ok(())
        }
        for r in n.roots.iter().filter(|r| **r >= 0) {
            go(&n, *r as usize, Transform::IDENTITY, Affine::IDENTITY, 0)?;
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
