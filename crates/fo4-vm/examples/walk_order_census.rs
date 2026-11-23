#![forbid(unsafe_code)]
//! The order a walk through an interior cell forces on a quest's stage
//! triggers (`spec/vm.md`, The museum's order; `spec/walk-order-census.txt`).
//!
//! ```text
//! walk_order_census MANIFEST CELL QUEST[,QUEST...] OUT.txt NAMES.txt
//!     --stage-property NAME [--prereq-property NAME] [--off-property NAME]
//!     [--body HEIGHT] [--door-margin UNITS]
//! ```
//!
//! CELL and each QUEST as `plugin.esm:0001ABCD`; a trigger is named by the
//! position of its quest in the list ("quest 1"). The property names are the
//! caller's: the integer properties of the trigger scripts that hold the
//! stage a trigger sets, the stage it needs done and the stage from which
//! it is turned off (the repository names no game script). OUT.txt holds
//! roles and counts only; NAMES.txt names the records and is never
//! versioned. Nothing runs.
//!
//! The walkable surface is the cell's navigation meshes: triangles joined
//! across their shared edges and across edge links of kind 0 (as
//! `track_plan` walks; kinds 1 and 2 in a separate reading). The player is
//! a vertical segment of HEIGHT units (default 120) standing on the
//! surface (`world_trigger_body_v1`). A volume blocks the surface where
//! the standing player would meet it:
//!
//! - a stage trigger: the activator reference's primitive (box or sphere)
//!   in its placement, as the run reads it (`world_trigger_box_v1`,
//!   `world_trigger_sphere_v1`);
//! - a door: the oriented box of its base's object bounds in its
//!   placement grown by the caller's margin (`walk_door_bounds_block_v1`),
//!   for the doors whose reference stores a lock record (`XLOC`) in the
//!   reading where locked doors are closed
//!   (`walk_door_lock_record_closed_v1`).
//!
//! The geometry is `fo4_vm::walk_order`: a triangle minus one convex
//! volume falls into as many pieces as the free arcs of its boundary, so
//! each triangle becomes a node per free arc and the pieces are joined
//! across the free parts of shared edges (exact for one volume per
//! triangle; triangles where two volumes meet the boundary are counted). From each entry (the arrival position of a
//! load door leading into the cell) the census says which stage triggers
//! the walk can reach with every door open and with the locked doors
//! closed, which single door opens a trigger that is shut, and, for each
//! reachable trigger, which other triggers every walk to it enters first.
use fo4_core::{Budget, Manifest};
use fo4_nav::{Edge, NavmeshRecord};
use fo4_plugin::{
    actor::Context,
    quest::{AliasValue, Quest},
    typed::{Refr, Store},
    vmad::{PropertyValue, Vmad},
    FormKey, Record,
};
use fo4_scene::transform::Transform;
use fo4_vm::{
    bind,
    track::{self, Shape, Switches, Volume},
    walk_order::{self as wo, Surface},
};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;

/// One blocking volume of the census.
struct Obstacle {
    /// Role in the versioned output.
    role: String,
    vol: Volume,
    /// A door with a lock record (closed in the locked reading).
    locked: bool,
    /// A stage trigger: (its quest's position, its stage).
    stage: Option<(usize, i64)>,
    /// A door: its bounds before the margin.
    bare: Option<Volume>,
}

fn obnd(store: &Store, base: &FormKey, b: &Budget) -> Option<([f64; 3], [f64; 3])> {
    let r = store.raw(base, b).ok()??;
    let s = r.subrecords.iter().find(|s| s.signature == *b"OBND")?;
    let d = r.data(s);
    if d.len() != 12 {
        return None;
    }
    let w = |i: usize| f64::from(i16::from_le_bytes([d[2 * i], d[2 * i + 1]]));
    Some(([w(0), w(1), w(2)], [w(3), w(4), w(5)]))
}

/// The integer properties of the reference's script whose properties name
/// one of the quests, and that quest's position.
fn script_ints(
    store: &Store,
    r: &Record,
    plugin: usize,
    quests: &[FormKey],
    b: &Budget,
) -> Option<(usize, BTreeMap<String, i64>)> {
    let s = r.subrecords.iter().find(|s| s.signature == *b"VMAD")?;
    let v = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b).ok()?;
    for sc in &v.scripts {
        let named = sc.properties.iter().find_map(|p| match &p.value {
            PropertyValue::Object(o) => {
                let k = store
                    .order()
                    .remap(plugin, o.form_id, &r.source.file, 0)
                    .ok()
                    .flatten()?;
                quests.iter().position(|q| *q == k)
            }
            _ => None,
        });
        if let Some(qi) = named {
            let ints = sc
                .properties
                .iter()
                .filter_map(|p| match p.value {
                    PropertyValue::Int(i) => Some((
                        String::from_utf8_lossy(p.name).to_ascii_lowercase(),
                        i64::from(i),
                    )),
                    _ => None,
                })
                .collect();
            return Some((qi, ints));
        }
    }
    None
}

/// The cell's navigation meshes as one surface, with the edge links of
/// the kinds given followed.
fn surface(
    store: &Store,
    cell: &FormKey,
    kinds: &[u32],
    b: &Budget,
) -> Result<(Surface, BTreeMap<String, u64>), E> {
    let ch = store.children(cell, b)?;
    let mut meshes = Vec::new();
    for kid in ch.excluded.iter().filter(|k| k.signature == *b"NAVM") {
        let rb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&kid.key, &rb)? else {
            continue;
        };
        meshes.push((
            kid.key.clone(),
            plugin,
            NavmeshRecord::decode(&r, &rb)?.mesh,
        ));
    }
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut s = Surface::default();
    let mut base = Vec::new();
    for (_, _, m) in &meshes {
        base.push(s.tris.len());
        *counts.entry("0 navigation meshes".into()).or_default() += 1;
        *counts.entry("0 triangles".into()).or_default() += m.triangles.len() as u64;
        *counts
            .entry("0 navigation mesh door entries".into())
            .or_default() += m.doors.len() as u64;
        for t in 0..m.triangles.len() {
            s.tris.push(
                m.corners(t)
                    .map(|c| c.map(|v| v.map(f64::from)))
                    .unwrap_or([[0.0; 3]; 3]),
            );
        }
    }
    let index: BTreeMap<&FormKey, usize> =
        meshes.iter().enumerate().map(|(i, m)| (&m.0, i)).collect();
    for (mi, (key, plugin, m)) in meshes.iter().enumerate() {
        let (nt, nl) = (m.triangles.len(), m.edge_links.len());
        for l in &m.edge_links {
            *counts
                .entry(format!("0 edge links of kind {}", l.kind))
                .or_default() += 1;
        }
        for t in 0..nt {
            let mut next = [None; 3];
            for (e, slot) in next.iter_mut().enumerate() {
                match m.triangles[t].edge(e, nt, nl) {
                    Edge::Triangle(u) => *slot = Some(base[mi] + usize::from(u)),
                    Edge::Link(k) => {
                        let l = m.edge_links[usize::from(k)];
                        if !kinds.contains(&l.kind) {
                            continue;
                        }
                        let Some(j) = store
                            .order()
                            .remap(*plugin, l.navmesh, &key.plugin, 0)
                            .ok()
                            .flatten()
                            .and_then(|k| index.get(&k).copied())
                        else {
                            continue;
                        };
                        if usize::from(l.triangle) < meshes[j].2.triangles.len() {
                            s.links.push((
                                (base[mi] + t, e),
                                (base[j] + usize::from(l.triangle), usize::from(l.edge) % 3),
                            ));
                        }
                    }
                    _ => {}
                }
            }
            s.next.push(next);
        }
    }
    Ok((s, counts))
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |flag: &str| -> Result<Option<String>, E> {
        match a.iter().position(|x| x == flag) {
            Some(i) => {
                let v = a.get(i + 1).cloned().ok_or(format!("{flag} VALUE"))?;
                a.drain(i..i + 2);
                Ok(Some(v))
            }
            None => Ok(None),
        }
    };
    let stage_p = opt("--stage-property")?
        .ok_or("--stage-property NAME is required")?
        .to_ascii_lowercase();
    let prereq_p = opt("--prereq-property")?.map(|s| s.to_ascii_lowercase());
    let off_p = opt("--off-property")?.map(|s| s.to_ascii_lowercase());
    let h: f64 = opt("--body")?
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(120.0);
    let margin: f64 = opt("--door-margin")?
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(0.0);
    if a.len() != 5 {
        return Err("walk_order_census MANIFEST CELL QUEST[,QUEST...] OUT.txt NAMES.txt --stage-property NAME [--prereq-property NAME] [--off-property NAME] [--body HEIGHT] [--door-margin UNITS]".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let cell = fo4_vm::player::parse_key(&a[1]).ok_or("CELL as plugin.esm:0001ABCD")?;
    let quests: Vec<FormKey> = a[2]
        .split(',')
        .map(|q| fo4_vm::player::parse_key(q).ok_or("QUEST as plugin.esm:0001ABCD"))
        .collect::<Result<_, _>>()?;
    let mut out = String::new();
    let mut names = String::new();
    let _ = writeln!(out, "# walk_order_census: roles and counts only (spec/vm.md, The museum's order). The cell's navigation meshes as the walkable surface (walk_navmesh_surface_v1), the player a vertical segment of {h} units; the stage triggers of {} quest(s) and the cell's doors, their bounds grown by {margin} units (walk_door_bounds_block_v1), as blocking volumes; a door storing a lock record closed in the locked reading (walk_door_lock_record_closed_v1).", quests.len());
    let _ = writeln!(out, "# Sections: 0 counts; 1 the stage triggers; 2 the doors (the smallest margin at which each alone splits the surface); 3 entries; 4 reach with every door open and with the locked doors closed; 5 the door that opens each shut trigger; 6 the triggers every walk to a trigger enters first; 7 the pieces of the surface with the locked doors closed.");
    // The first quest's aliases holding a placed reference by force.
    let mut alias_of: BTreeMap<FormKey, Vec<u32>> = BTreeMap::new();
    if let Some((r, plugin)) = store.raw_with_plugin(&quests[0], &b)? {
        let q = Quest::read(
            &r,
            Context {
                order: store.order(),
                plugin,
            },
            &b,
        )?;
        for al in &q.aliases {
            for v in &al.values {
                if let AliasValue::ForcedReference(f) = v {
                    if let Some(k) = &f.key {
                        alias_of.entry(k.clone()).or_default().push(al.id);
                    }
                }
            }
        }
    }
    let (plain_surface, mut counts) = surface(&store, &cell, &[0], &b)?;
    let (jump_surface, _) = surface(&store, &cell, &[0, 1, 2], &b)?;
    let sw = Switches {
        boxes: true,
        spheres: true,
        ..Switches::default()
    };
    let mut obs: Vec<Obstacle> = Vec::new();
    let mut entries: Vec<(String, String, [f64; 3])> = Vec::new();
    let ch = store.children(&cell, &b)?;
    for kid in ch.active.iter().chain(&ch.disabled) {
        if kid.signature != *b"REFR" {
            continue;
        }
        let rb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&kid.key, &rb)? else {
            continue;
        };
        let Some(base) = bind::base_of(&store, &kid.key, &rb)? else {
            continue;
        };
        let Some(refr) = store.get::<Refr>(&kid.key, &rb)? else {
            continue;
        };
        let Some(t) = refr.data.transform else {
            continue;
        };
        let scale = f64::from(refr.data.scale.unwrap_or(1.0));
        let Ok(frame) = Transform::euler(t, 1.0) else {
            continue;
        };
        let key = bind::record_key(&kid.key);
        let volume = |frame: Transform, shape: Shape| Volume {
            key: kid.key.clone(),
            space: cell.clone(),
            frame,
            shape,
        };
        match store.signature(&base).as_ref() {
            Some(b"ACTI") => {
                let Ok(Some(p)) = track::primitive_of(&r) else {
                    continue;
                };
                let Some((qi, ints)) = script_ints(&store, &r, plugin, &quests, &rb) else {
                    continue;
                };
                let Some(&stage) = ints.get(&stage_p) else {
                    continue;
                };
                let Ok(shape) = sw.shape(&p, scale) else {
                    continue;
                };
                let mut role = format!("quest {}'s stage {stage} trigger", qi + 1);
                if let Some(v) = prereq_p
                    .as_ref()
                    .and_then(|n| ints.get(n))
                    .filter(|v| **v >= 0)
                {
                    let _ = write!(role, " (needs {v})");
                }
                if let Some(v) = off_p
                    .as_ref()
                    .and_then(|n| ints.get(n))
                    .filter(|v| **v >= 0)
                {
                    let _ = write!(role, " (off from {v})");
                }
                *counts
                    .entry(format!(
                        "0 stage triggers in the cell\tquest {}\t{}",
                        qi + 1,
                        match shape {
                            Shape::Box(_) => "box",
                            Shape::Sphere(_) => "sphere",
                        }
                    ))
                    .or_default() += 1;
                let _ = writeln!(names, "{role}: {key} at {t:?} primitive {:?}", p.bounds);
                obs.push(Obstacle {
                    role,
                    vol: volume(frame, shape),
                    locked: false,
                    stage: Some((qi, stage)),
                    bare: None,
                });
            }
            Some(b"DOOR") => {
                let tp = track::teleport_of(&store, &r, plugin).ok().flatten();
                let lock = r
                    .subrecords
                    .iter()
                    .find(|s| s.signature == *b"XLOC")
                    .map(|s| r.data(s).first().copied().unwrap_or(0));
                let Some((lo, hi)) = obnd(&store, &base, &rb) else {
                    *counts
                        .entry("0 doors left out\tno object bounds".into())
                        .or_default() += 1;
                    continue;
                };
                let centre: [f64; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0 * scale);
                let half: [f64; 3] = std::array::from_fn(|i| (hi[i] - lo[i]) / 2.0 * scale);
                let mut f = frame;
                f.translation = frame.apply(centre);
                let mut role = format!(
                    "{} door{}",
                    if tp.is_some() { "a load" } else { "an inner" },
                    match lock {
                        Some(l) => format!(" with a lock record (level byte {l})"),
                        None => String::new(),
                    }
                );
                if let Some(ids) = alias_of.get(&kid.key) {
                    let _ = write!(role, " held by quest 1's alias {ids:?}");
                }
                *counts.entry(format!("0 doors\t{role}")).or_default() += 1;
                let _ = writeln!(names, "{role}: {key} at {t:?} bounds {lo:?} {hi:?}");
                if let Some(tp) = &tp {
                    // The arrival here: the other side's teleport position.
                    let ob = b.child();
                    if let Some((or, op)) = store.raw_with_plugin(&tp.door, &ob)? {
                        if let Ok(Some(back)) = track::teleport_of(&store, &or, op) {
                            let from = store.placement_groups().find_map(|(o, c, _)| {
                                (store.overrides().form_key(o.key, store.order()) == tp.door)
                                    .then_some(c)
                            });
                            let kind = match from.as_ref().and_then(|c| store.world(c)) {
                                Some(_) => "from an exterior space",
                                None => "from an interior space",
                            };
                            entries.push((
                                format!("the arrival through {role} ({kind})"),
                                format!("{key} <- {}", bind::record_key(&tp.door)),
                                back.pos,
                            ));
                        }
                    }
                }
                let bare = volume(f, Shape::Box(half));
                obs.push(Obstacle {
                    role,
                    vol: wo::grown(&bare, margin),
                    locked: lock.is_some(),
                    stage: None,
                    bare: Some(bare),
                });
            }
            _ => {}
        }
    }
    // Stable order: triggers by quest and stage, then doors by role.
    obs.sort_by(|x, y| {
        (x.stage.is_none(), x.stage, &x.role, &x.vol.key.object_id).cmp(&(
            y.stage.is_none(),
            y.stage,
            &y.role,
            &y.vol.key.object_id,
        ))
    });
    entries.sort_by(|x, y| (&x.0, &x.1).cmp(&(&y.0, &y.1)));
    // Repeated roles told apart by their place in that order.
    let mut seen: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for o in &obs {
        seen.entry(o.role.clone()).or_default().1 += 1;
    }
    for o in &mut obs {
        let e = seen.get_mut(&o.role).expect("counted");
        if e.1 > 1 {
            e.0 += 1;
            o.role = format!("{} ({} of {})", o.role, e.0, e.1);
        }
    }
    let triggers: Vec<usize> = (0..obs.len()).filter(|&i| obs[i].stage.is_some()).collect();
    let locked: Vec<&Volume> = obs.iter().filter(|o| o.locked).map(|o| &o.vol).collect();
    for (k, v) in &counts {
        let (sec, rest) = k.split_once(' ').unwrap_or(("", k));
        let _ = writeln!(out, "{v}\t{sec} {rest}");
    }
    let plain = wo::split(&plain_surface, &[], h).sizes().len();
    let _ = writeln!(
        out,
        "{plain}\t0 pieces of the surface with nothing blocking"
    );
    // 1, 2: the volumes, the triangles they meet, whether each alone splits.
    for o in &obs {
        let mut met = 0u64;
        let mut whole = 0u64;
        for c in &plain_surface.tris {
            whole += u64::from((0..3).all(|k| wo::inside(&o.vol, c[k], h)));
            met += u64::from(wo::meets(&o.vol, c, h, 4.0));
        }
        let sz = wo::split(&plain_surface, &[&o.vol], h).sizes();
        let sec = if o.stage.is_some() { 1 } else { 2 };
        let mut line = format!(
            "{sec} {}\ttriangles met {met}, wholly inside {whole}\tthe surface without it: {} pieces, the largest {:?}",
            o.role,
            sz.len(),
            &sz[..sz.len().min(4)]
        );
        if let Some(bare) = o.bare.as_ref().filter(|_| o.locked || met > 0) {
            let at = [0.0, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0]
                .into_iter()
                .find(|mg| {
                    wo::split(&plain_surface, &[&wo::grown(bare, *mg)], h)
                        .sizes()
                        .len()
                        > plain
                });
            let _ = write!(
                line,
                "\tsplits the surface from a margin of {}",
                at.map(|m| format!("{m} units"))
                    .unwrap_or("none up to 32 units".into())
            );
        }
        let _ = writeln!(out, "{line}");
    }
    // 3: entries.
    for (role, name, p) in &entries {
        let _ = writeln!(out, "3 entry: {role}");
        let _ = writeln!(names, "entry {role}: {name} at {p:?}");
    }
    // 4, 5, 6 per entry and reading.
    for (role, _, p) in &entries {
        for (reading, s) in [
            ("edge links of kind 0", &plain_surface),
            ("edge links of kinds 0, 1 and 2", &jump_surface),
        ] {
            let tag = format!("from {role}, {reading}");
            let first_too = reading.ends_with("kind 0");
            let none: Vec<&Volume> = Vec::new();
            for (state, blocking) in [
                ("every door open", &none),
                ("the locked doors closed", &locked),
            ] {
                for &t in &triggers {
                    let r = wo::reach(s, blocking, *p, &obs[t].vol, h);
                    let _ = writeln!(
                        out,
                        "4 {tag}, {state}\t{}\t{}",
                        obs[t].role,
                        match r {
                            Some(true) => "reached",
                            Some(false) => "shut",
                            None => "the entry stands on no free surface",
                        }
                    );
                    if r == Some(false) && !blocking.is_empty() && first_too {
                        // 5: the single locked door that, opened, lets it be reached.
                        let opens: Vec<String> = obs
                            .iter()
                            .filter(|o| o.locked)
                            .filter(|d| {
                                let rest: Vec<&Volume> = obs
                                    .iter()
                                    .filter(|o| o.locked && !std::ptr::eq(*o, *d))
                                    .map(|o| &o.vol)
                                    .collect();
                                wo::reach(s, &rest, *p, &obs[t].vol, h) == Some(true)
                            })
                            .map(|d| d.role.clone())
                            .collect();
                        let _ = writeln!(
                            out,
                            "5 {tag}\t{}\topened by: {}",
                            obs[t].role,
                            if opens.is_empty() {
                                "no single door".to_string()
                            } else {
                                opens.join("; ")
                            }
                        );
                    }
                    if r != Some(true) || !first_too {
                        continue;
                    }
                    // 6: the other triggers every walk to it enters first.
                    let others: Vec<usize> = triggers.iter().copied().filter(|u| *u != t).collect();
                    let vols: Vec<&Volume> = others.iter().map(|u| &obs[*u].vol).collect();
                    let first: Vec<String> =
                        wo::entered_first(s, blocking, *p, &obs[t].vol, &vols, h)
                            .into_iter()
                            .map(|i| obs[others[i]].role.clone())
                            .collect();
                    let _ = writeln!(
                        out,
                        "6 {tag}, {state}\t{}\tentered first: {}",
                        obs[t].role,
                        if first.is_empty() {
                            "none".to_string()
                        } else {
                            first.join("; ")
                        }
                    );
                }
            }
            let multi = wo::split(s, &locked, h).multi;
            let _ = writeln!(
                out,
                "0 {tag}: triangles whose boundary two blocking volumes meet with the locked doors closed\t{multi}"
            );
        }
    }
    // 7: the pieces with the locked doors closed: their size, the triggers
    // they meet, the locked doors beside them, the entries on them.
    let mut sp = wo::split(&plain_surface, &locked, h);
    let size = sp.by_root();
    let mut what: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for &t in &triggers {
        for r in wo::meeting(&plain_surface, &mut sp, &locked, &obs[t].vol, h) {
            what.entry(r).or_default().push(obs[t].role.clone());
        }
    }
    for d in obs.iter().filter(|o| o.locked) {
        let near = wo::grown(&d.vol, 8.0);
        for r in wo::meeting(&plain_surface, &mut sp, &locked, &near, h) {
            what.entry(r)
                .or_default()
                .push(format!("beside {}", d.role));
        }
    }
    for (role, _, p) in &entries {
        if let Some(r) = wo::entry(&plain_surface, &mut sp, &locked, *p, h) {
            what.entry(r).or_default().push(format!("entry: {role}"));
        }
    }
    let mut rows: Vec<(u64, String)> = what
        .iter()
        .map(|(r, v)| (size.get(r).copied().unwrap_or(0), v.join("; ")))
        .collect();
    rows.sort_by(|x, y| y.cmp(x));
    for (n, v) in rows {
        let _ = writeln!(out, "7 a piece of {n} triangle pieces\t{v}");
    }
    std::fs::write(&a[3], out)?;
    std::fs::write(&a[4], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("walk_order_census: {e}");
        std::process::exit(1);
    }
}
