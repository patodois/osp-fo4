#![forbid(unsafe_code)]
//! Census of the root node's own transform in the models the static renderer
//! draws, for `spec/nif.md` (Root node transform). For every winning NIF whose
//! root node has a non-identity rotation, translation or scale, the model box
//! is composed as the renderer composes it (row reading of
//! `fo4_scene::transform::Transform`, editor-marker subtrees, outside and
//! state blocks and skinned shapes left out) under five handlings of the
//! root's own transform: applied, left out, and left out in one part only
//! (rotation, translation or scale). Each box is compared with the OBND of
//! every base record that names the model. Second witness, for the models
//! whose collision object targets a chain that starts at such a root: the
//! stored body placement against the root transform, and the collision box
//! (chain composed with and without the root, units of
//! `havok_collision_units_69_99125_v1`, body placement ignored) against the
//! render box of the Target subtree with and without the root.
//!
//! `root_transform_census MANIFEST OUT.txt [MODELS.tsv]`; the optional table
//! names models and is kept outside the repository.
use fo4_core::{Budget, Game, Manifest};
use fo4_nif::{Data, Nif, Value};
use fo4_plugin::typed::Store;
use fo4_scene::{model_policy, transform::Transform};
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

/// An OBND (six i16) and the signature of its record.
type Obnd = ([i16; 6], [u8; 4]);

#[derive(Clone, Copy, Debug)]
struct Box3 {
    min: [f64; 3],
    max: [f64; 3],
}
impl Box3 {
    fn add(this: &mut Option<Self>, p: [f64; 3]) {
        match this {
            None => *this = Some(Self { min: p, max: p }),
            Some(b) => {
                for ((lo, hi), v) in b.min.iter_mut().zip(b.max.iter_mut()).zip(p) {
                    *lo = lo.min(v);
                    *hi = hi.max(v);
                }
            }
        }
    }
    fn err(&self, o: &Self) -> f64 {
        (0..3)
            .map(|i| {
                (self.min[i] - o.min[i])
                    .abs()
                    .max((self.max[i] - o.max[i]).abs())
            })
            .fold(0., f64::max)
    }
}

/// The five handlings of the root's own transform.
const VARIANTS: [&str; 5] = [
    "applied",
    "left out",
    "rotation left out",
    "translation left out",
    "scale left out",
];
fn variant(root: Transform, v: usize) -> Transform {
    match v {
        0 => root,
        1 => Transform::IDENTITY,
        2 => Transform {
            rotation: Transform::IDENTITY.rotation,
            ..root
        },
        3 => Transform {
            translation: [0.; 3],
            ..root
        },
        _ => Transform { scale: 1., ..root },
    }
}
fn kind(t: &Transform) -> String {
    let r = (0..3)
        .any(|i| (0..3).any(|j| (t.rotation[i][j] - if i == j { 1. } else { 0. }).abs() > 1e-5));
    let tr = t.translation.iter().any(|x| x.abs() > 1e-3);
    let s = (t.scale - 1.).abs() > 1e-5;
    let mut k = String::new();
    if r {
        k.push('R');
    }
    if tr {
        k.push('T');
    }
    if s {
        k.push('S');
    }
    if k.is_empty() {
        k.push('-');
    }
    k
}
/// Inverse of a similarity with orthonormal rows.
fn inverse(t: &Transform) -> Transform {
    let rt: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| t.rotation[j][i]));
    let s = 1. / t.scale;
    let p = Transform {
        rotation: rt,
        translation: [0.; 3],
        scale: s,
    }
    .apply(t.translation);
    Transform {
        rotation: rt,
        translation: p.map(|v| -v),
        scale: s,
    }
}

struct Walker<'a, 'n> {
    n: &'a Nif<'n>,
    visits: u64,
    boxes: [Option<Box3>; 5],
}
impl Walker<'_, '_> {
    fn visit(&mut self, i: usize, below: [Transform; 5], depth: u32) {
        self.visits += 1;
        if depth > 64 || self.visits > 1_000_000 {
            return;
        }
        let Some(block) = self.n.blocks.get(i) else {
            return;
        };
        let ty = &self.n.block_types[block.type_index as usize];
        if model_policy::visibility_editor_marker(self.n, i)
            || model_policy::editor_marker_object(self.n, i)
            || model_policy::editor_marker_suffix(self.n, i)
        {
            return;
        }
        if matches!(
            model_policy::block_class(ty),
            fo4_plugin::static_frame::Class::Outside | fo4_plugin::static_frame::Class::State
        ) {
            return;
        }
        let Some(data) = block.decoded.as_ref() else {
            return;
        };
        let f = model_policy::fields(data);
        let mut t = below;
        if f.iter().any(|f| f.name == "Translation") {
            let Ok(local) = Transform::from_fields(f, "census") else {
                return;
            };
            for (k, x) in t.iter_mut().enumerate() {
                *x = if depth == 0 {
                    x.compose(variant(local, k))
                } else {
                    x.compose(local)
                };
            }
        }
        match data {
            Data::Node { children, .. } => {
                for ch in children.iter().filter(|c| **c >= 0) {
                    self.visit(*ch as usize, t, depth + 1);
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
                    return;
                };
                if !link(f, "Skin").is_empty() || !link(f, "Skin Instance").is_empty() {
                    return;
                }
                for v in mesh.vertices.iter().flatten() {
                    let Some(p) = v.position else { continue };
                    let p = p.map(f64::from);
                    for (k, x) in t.iter().enumerate() {
                        Box3::add(&mut self.boxes[k], x.apply(p));
                    }
                }
            }
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("root_transform_census MANIFEST OUT.txt [MODELS.tsv]".into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    // Base records with a model path and an OBND, by model path.
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut obnd: BTreeMap<Vec<u8>, Vec<Obnd>> = BTreeMap::new();
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
    let (mut files, mut failed) = (0u64, 0u64);
    let mut roots_by_kind: BTreeMap<String, [u64; 2]> = BTreeMap::new();
    // Per root kind: base records by the set of handlings whose box is
    // within 1.01 of the OBND (bit k for VARIANTS[k]), over models whose
    // five boxes are not all within 1 of each other.
    let mut fits: BTreeMap<String, BTreeMap<u32, u64>> = BTreeMap::new();
    // Per root kind: base records by the handling with the smallest error
    // (ties within 0.01 listed together).
    let mut best: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut same_boxes: BTreeMap<String, u64> = BTreeMap::new();
    // Collision witness.
    let mut body_vs_root: BTreeMap<String, u64> = BTreeMap::new();
    let mut col_fit: BTreeMap<String, u64> = BTreeMap::new();
    let mut table = String::from("path\troot_kind\tbase_records\tfit_masks\tbest\n");
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
        // The first root node's own transform.
        let Some(root) = n.roots.iter().find(|r| **r >= 0).map(|r| *r as usize) else {
            continue;
        };
        let Some(Data::Node { fields, .. }) = n.blocks.get(root).and_then(|x| x.decoded.as_ref())
        else {
            continue;
        };
        let Ok(rt) = Transform::from_fields(fields, &name) else {
            continue;
        };
        let k = kind(&rt);
        let named = obnd.contains_key(path);
        let e = roots_by_kind.entry(k.clone()).or_default();
        e[0] += 1;
        e[1] += u64::from(named);
        if k == "-" {
            continue;
        }
        let mut w = Walker {
            n: &n,
            visits: 0,
            boxes: [None; 5],
        };
        for r in n.roots.iter().filter(|r| **r >= 0) {
            w.visit(*r as usize, [Transform::IDENTITY; 5], 0);
        }
        let boxes = w.boxes;
        let mut masks = Vec::new();
        let mut bests = Vec::new();
        if let (Some(recs), Some(b0)) = (obnd.get(path), boxes[0]) {
            let all: Vec<Box3> = boxes.iter().flatten().copied().collect();
            if all.len() == 5 && all.iter().all(|x| x.err(&b0) <= 1.) {
                *same_boxes.entry(k.clone()).or_default() += recs.len() as u64;
            } else if all.len() == 5 {
                for (o, _) in recs {
                    let ob = Box3 {
                        min: [o[0], o[1], o[2]].map(f64::from),
                        max: [o[3], o[4], o[5]].map(f64::from),
                    };
                    let errs: Vec<f64> = all.iter().map(|x| x.err(&ob)).collect();
                    let mask = errs
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| **e <= 1.01)
                        .fold(0u32, |m, (i, _)| m | 1 << i);
                    bump(fits.entry(k.clone()).or_default(), mask);
                    let min = errs.iter().copied().fold(f64::INFINITY, f64::min);
                    let who: Vec<&str> = errs
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| **e <= min + 0.01)
                        .map(|(i, _)| VARIANTS[i])
                        .collect();
                    let who = who.join(" = ");
                    bump(best.entry(k.clone()).or_default(), who.clone());
                    masks.push(mask);
                    bests.push(who);
                }
            }
        }
        if named {
            let _ = writeln!(
                table,
                "{name}\t{k}\t{}\t{masks:?}\t{}",
                obnd.get(path).map_or(0, Vec::len),
                bests.join("; ")
            );
        }
        // Collision witness.
        let keep = Budget::new(m.limits.max_memory);
        let Ok(col) = fo4_scene::collision::model::decode(&n, 0, &name, &m.limits, &keep) else {
            continue;
        };
        for body in &col.bodies {
            if body.chain.first() != Some(&root) {
                continue;
            }
            let bp = &body.body;
            let ident = fo4_scene::collision::body_placement_is_identity(bp);
            let q = fo4_scene::collision::quaternion_rows(bp.rotation);
            let rot_eq = (0..3).all(|i| (0..3).all(|j| (q[i][j] - rt.rotation[i][j]).abs() < 1e-3));
            let rot_tr = (0..3).all(|i| (0..3).all(|j| (q[i][j] - rt.rotation[j][i]).abs() < 1e-3));
            let tr_eq = (0..3).all(|i| {
                (f64::from(bp.translation[i]) * fo4_scene::collision::UNITS - rt.translation[i])
                    .abs()
                    < 0.5
            });
            bump(
                &mut body_vs_root,
                format!(
                    "root {k}: body placement {}",
                    if ident {
                        "identity"
                    } else if rot_eq && tr_eq {
                        "equal to the root's rotation and translation"
                    } else if rot_eq {
                        "rotation equal to the root's"
                    } else if rot_tr {
                        "rotation equal to the root's transposed"
                    } else {
                        "other"
                    }
                ),
            );
            // Collision box: chain with the root (as placed today) and
            // without it, body placement ignored.
            let units = Transform {
                scale: fo4_scene::collision::UNITS,
                ..Transform::IDENTITY
            };
            let with_root = body.target_local.compose(units);
            let without_root = inverse(&rt).compose(body.target_local).compose(units);
            let mut cb: [Option<Box3>; 2] = [None, None];
            for leaf in &body.leaves {
                let pts: Vec<[f32; 3]> = match &leaf.shape {
                    fo4_scene::collision::Shape::Mesh { vertices, .. }
                    | fo4_scene::collision::Shape::Polytope { vertices, .. }
                    | fo4_scene::collision::Shape::Convex { vertices } => vertices.clone(),
                    fo4_scene::collision::Shape::Sphere { center, .. } => vec![*center],
                    fo4_scene::collision::Shape::Capsule { points, .. } => points.to_vec(),
                };
                for p in pts {
                    let q = leaf.to_body(p);
                    Box3::add(&mut cb[0], with_root.apply(q));
                    Box3::add(&mut cb[1], without_root.apply(q));
                }
            }
            let mut rb: [Option<Box3>; 2] = [None, None];
            let inv = inverse(&rt);
            for p in col.render_points(body) {
                Box3::add(&mut rb[0], p);
                Box3::add(&mut rb[1], inv.apply(p));
            }
            if let ([Some(c0), Some(c1)], [Some(r0), Some(r1)]) = (cb, rb) {
                // Agreement within 10 units or 10 % of the render box's
                // largest side, for each pairing.
                let side = (0..3)
                    .map(|i| r0.max[i] - r0.min[i])
                    .fold(0., f64::max)
                    .max((0..3).map(|i| r1.max[i] - r1.min[i]).fold(0., f64::max));
                let tol = (0.1 * side).max(10.);
                let pairs = [
                    ("collision and render both with the root", c0.err(&r0)),
                    ("collision with, render without", c0.err(&r1)),
                    ("collision without, render with", c1.err(&r0)),
                ];
                let agree: Vec<&str> = pairs
                    .iter()
                    .filter(|(_, e)| *e <= tol)
                    .map(|(n, _)| *n)
                    .collect();
                bump(
                    &mut col_fit,
                    format!(
                        "root {k}: {}",
                        if agree.is_empty() {
                            "none".into()
                        } else {
                            agree.join(" | ")
                        }
                    ),
                );
            }
        }
    }
    let mut out = String::new();
    writeln!(out, "# root_transform_census")?;
    writeln!(
        out,
        "# root kinds: R rotation, T translation, S scale (- identity); handlings: {}",
        VARIANTS.join(", ")
    )?;
    writeln!(out, "files\t{files} read, {failed} not read")?;
    for (k, [a, n]) in &roots_by_kind {
        writeln!(
            out,
            "root {k}\t{a} files, {n} named by a base record with MODL and OBND"
        )?;
    }
    for (k, n) in &same_boxes {
        writeln!(
            out,
            "root {k}\tbase records whose five boxes agree within 1 (decide nothing)\t{n}"
        )?;
    }
    writeln!(
        out,
        "# OBND within 1.01, by set of handlings (bits in the order above), base records"
    )?;
    for (k, masks) in &fits {
        let v: Vec<String> = masks
            .iter()
            .map(|(m, n)| {
                let names: Vec<&str> = (0..5)
                    .filter(|i| m & (1 << i) != 0)
                    .map(|i| VARIANTS[i])
                    .collect();
                format!(
                    "[{}]:{n}",
                    if names.is_empty() {
                        "none".into()
                    } else {
                        names.join("+")
                    }
                )
            })
            .collect();
        writeln!(out, "root {k}\t{}", v.join(" "))?;
    }
    writeln!(
        out,
        "# handling with the smallest OBND error (ties within 0.01), base records"
    )?;
    for (k, m) in &best {
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let v: Vec<String> = v.iter().map(|(w, n)| format!("[{w}]:{n}")).collect();
        writeln!(out, "root {k}\t{}", v.join(" "))?;
    }
    writeln!(
        out,
        "# collision bodies whose Target chain starts at a non-identity root"
    )?;
    for (k, n) in &body_vs_root {
        writeln!(out, "{k}\t{n}")?;
    }
    writeln!(
        out,
        "# same bodies: collision box against the render box of the Target subtree (within 10 or 10% of the largest side)"
    )?;
    for (k, n) in &col_fit {
        writeln!(out, "{k}\t{n}")?;
    }
    std::fs::write(&args[1], out)?;
    if let Some(t) = args.get(2) {
        std::fs::write(t, table)?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
