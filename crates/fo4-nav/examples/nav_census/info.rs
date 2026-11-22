//! The NAVI pass: every occurrence read to the last byte, each NVMI entry
//! compared with the mesh it names.
use crate::meshes::Summary;
use crate::stats::{add, bump, check, checks, counts, top, Checks, Counts};
use fo4_core::{Budget, Manifest};
use fo4_nav::NavInfo;
use fo4_plugin::typed::Store;
use fo4_plugin::{FormKey, Record};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

fn resolve(store: &Store, plugin: usize, id: u32, r: &Record, off: u64) -> Option<FormKey> {
    store
        .order()
        .remap(plugin, id, &r.source.file, off)
        .ok()
        .flatten()
}

fn kind_of(store: &Store, k: &FormKey) -> String {
    match store.overrides().winner(k) {
        None => "no record".into(),
        Some(w) if w.deleted => format!("{} (deleted)", String::from_utf8_lossy(&w.signature)),
        Some(w) => String::from_utf8_lossy(&w.signature).into_owned(),
    }
}

pub fn run(
    m: &Manifest,
    store: &Store,
    summary: &Summary,
    out: &mut String,
    names: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut c = Counts::new();
    let mut chk = Checks::new();
    let mut raw: BTreeMap<String, Counts> = BTreeMap::new();
    let mut rawb = |f: &str, v: String| bump(raw.entry(f.into()).or_default(), v);
    let mut covered: HashMap<FormKey, u32> = HashMap::new();
    let mut covered_by_winner: BTreeSet<FormKey> = BTreeSet::new();
    let mut nvpp_bytes: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut keys: BTreeSet<FormKey> = BTreeSet::new();
    for (i, o) in store.overrides().history.iter().enumerate() {
        if o.signature != *b"NAVI" {
            continue;
        }
        let winner = store
            .overrides()
            .winner_id(o.key)
            .is_some_and(|w| std::ptr::eq(w, o));
        let role = if winner { "winning" } else { "overridden" };
        bump(&mut c, format!("NAVI occurrences: {role}"));
        keys.insert(store.overrides().form_key(o.key, store.order()));
        if o.deleted {
            bump(&mut c, "NAVI deleted");
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let (r, plugin) = store.raw_occurrence(i, &b)?;
        add(&mut c, "NAVI payload bytes", r.payload.len() as u64);
        let info = match NavInfo::decode(&r, &b) {
            Ok(x) => x,
            Err(e) => {
                bump(&mut c, format!("NAVI refused: {}", e.code));
                writeln!(names, "refused NAVI {e}")?;
                continue;
            }
        };
        bump(&mut c, "NAVI read to the last byte");
        add(&mut c, "NVMI entries", info.meshes.len() as u64);
        add(
            &mut c,
            format!("NVMI entries in the NAVI of plugin index {plugin}"),
            info.meshes.len() as u64,
        );
        if let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"NVPP") {
            nvpp_bytes.insert(r.data(s).to_vec());
        }
        let s_off = r.source.offset;
        for e in &info.meshes {
            rawb("NVMI flags", format!("{:#x}", e.flags));
            rawb(
                "NVMI f32 after the position",
                if e.word == 0.0 {
                    "zero".into()
                } else {
                    "non-zero".into()
                },
            );
            for (w, _) in &e.pairs {
                rawb("NVMI pair first word", format!("{w:#010x}"));
            }
            let k = resolve(store, plugin, e.navmesh, &r, s_off);
            let mi = k.as_ref().and_then(|k| summary.index.get(k));
            check(&mut chk, "NVMI names a winning live NAVM", mi.is_some());
            if let Some(k) = &k {
                *covered.entry(k.clone()).or_default() += 1;
                if winner {
                    covered_by_winner.insert(k.clone());
                }
            }
            let Some(mi) = mi else {
                bump(
                    &mut c,
                    format!(
                        "NVMI names: {}",
                        k.map_or("unresolved".into(), |k| kind_of(store, &k))
                    ),
                );
                continue;
            };
            let mesh = &summary.meshes[*mi];
            let w = resolve(store, plugin, e.world, &r, s_off);
            check(
                &mut chk,
                "NVMI world = the mesh's header world (both resolved)",
                w == mesh.world,
            );
            if e.world == 0 {
                let p = resolve(store, plugin, e.parent, &r, s_off);
                check(
                    &mut chk,
                    "NVMI parent (world zero) = the mesh's parent cell",
                    p.is_some() && p == mesh.parent_cell,
                );
            } else {
                check(
                    &mut chk,
                    "NVMI parent word (world set) = the mesh's grid word",
                    e.parent == mesh.parent,
                );
            }
            let inside = (0..3).all(|j| {
                e.position[j] >= mesh.min[j] - 1e-3 && e.position[j] <= mesh.max[j] + 1e-3
            });
            check(
                &mut chk,
                "NVMI position inside the mesh's vertex box",
                inside,
            );
            let first: BTreeSet<FormKey> = e
                .first
                .iter()
                .filter_map(|x| resolve(store, plugin, *x, &r, s_off))
                .collect();
            let second: BTreeSet<FormKey> = e
                .second
                .iter()
                .filter_map(|x| resolve(store, plugin, *x, &r, s_off))
                .collect();
            for x in first.iter().chain(&second) {
                rawb("NVMI list entries name", kind_of(store, x));
            }
            check(
                &mut chk,
                "NVMI first list = the meshes its edge links name",
                first == mesh.link_targets,
            );
            let empty = BTreeSet::new();
            let k0 = mesh.link_targets_by_kind.get(&0).unwrap_or(&empty);
            let k12: BTreeSet<FormKey> = mesh
                .link_targets_by_kind
                .iter()
                .filter(|(k, _)| **k != 0)
                .flat_map(|(_, v)| v.iter().cloned())
                .collect();
            check(
                &mut chk,
                "NVMI first list = other meshes named by kind-0 edge links",
                &first == k0,
            );
            check(
                &mut chk,
                "NVMI second list = other meshes named by kind-1 or kind-2 edge links",
                second == k12,
            );
            check(
                &mut chk,
                "NVMI navmesh has an occurrence in the plugin of this NAVI occurrence",
                mesh.plugins.contains(&plugin),
            );
            let union: BTreeSet<FormKey> = first.union(&second).cloned().collect();
            check(
                &mut chk,
                "NVMI first and second lists together = the meshes its edge links name",
                union == mesh.link_targets,
            );
            check(
                &mut chk,
                "NVMI second list disjoint from the first",
                first.is_disjoint(&second),
            );
            let doors: BTreeSet<FormKey> = e
                .pairs
                .iter()
                .filter_map(|(_, d)| resolve(store, plugin, *d, &r, s_off))
                .collect();
            check(
                &mut chk,
                "NVMI pair second words = the doors its door links name",
                doors == mesh.doors,
            );
            if let Some(isl) = &e.island {
                for t in &isl.triangles {
                    check(
                        &mut chk,
                        "island triangle vertex < island vertices",
                        t.iter().all(|v| usize::from(*v) < isl.vertices.len()),
                    );
                }
                for v in &isl.vertices {
                    check(
                        &mut chk,
                        "island vertex equals a vertex of the mesh",
                        mesh.vertices.contains(v),
                    );
                }
                let mut lo = [f32::INFINITY; 3];
                let mut hi = [f32::NEG_INFINITY; 3];
                for v in &isl.vertices {
                    for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(v) {
                        *l = l.min(*x);
                        *h = h.max(*x);
                    }
                }
                check(
                    &mut chk,
                    "island corners = its vertices' box",
                    lo == isl.min && hi == isl.max,
                );
                add(&mut c, "island triangles", isl.triangles.len() as u64);
                add(&mut c, "island vertices", isl.vertices.len() as u64);
            }
        }
        let p = &info.preferred;
        add(
            &mut c,
            format!("NVPP sets (plugin index {plugin})"),
            p.sets.len() as u64,
        );
        for set in &p.sets {
            for x in set {
                let k = resolve(store, plugin, *x, &r, s_off);
                rawb(
                    "NVPP set members name",
                    k.map_or("unresolved".into(), |k| kind_of(store, &k)),
                );
            }
        }
        for (x, idx) in &p.pairs {
            let k = resolve(store, plugin, *x, &r, s_off);
            rawb(
                "NVPP pair first word names",
                k.map_or("unresolved".into(), |k| kind_of(store, &k)),
            );
            check(
                &mut chk,
                "NVPP pair second word < set count",
                (*idx as usize) < p.sets.len(),
            );
        }
    }
    add(&mut c, "distinct NAVI FormIDs", keys.len() as u64);
    add(
        &mut c,
        "distinct NVPP payloads across occurrences",
        nvpp_bytes.len() as u64,
    );
    for k in summary.index.keys() {
        check(
            &mut chk,
            "winning live NAVM named by an NVMI of some NAVI occurrence",
            covered.contains_key(k),
        );
        check(
            &mut chk,
            "winning live NAVM named by an NVMI of the winning NAVI",
            covered_by_winner.contains(k),
        );
    }
    for n in covered.values() {
        bump(
            &mut c,
            format!("navmeshes named by NVMI in {n} NAVI occurrence(s)"),
        );
    }
    counts(out, "NAVI records", &c);
    for (k, m) in &raw {
        top(out, k, m, 24);
    }
    checks(out, "NAVI consistency checks", &chk);
    Ok(())
}
