#![forbid(unsafe_code)]
//! What the player position model (`fo4_vm::track`, `spec/vm.md`, Player
//! position) reads and how much of the data each modelled or unmodelled
//! kind covers: `world_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts and kinds only (`spec/world-census.txt`); NAMES.txt
//! names scripts and references and is never versioned. Nothing runs:
//!
//! 1. the `XPRM` subrecord of every winning placed reference: sizes, the
//!    kind word, the pattern of the bound words by kind, the colour and
//!    float words, the base's record type (the measured layout);
//! 2. the placed activators with a primitive: by kind, with a script
//!    declaring a trigger handler, enable state, scale and tilt;
//! 3. for the boxes of activators with a trigger handler and an upright
//!    frame, the box's bottom against the highest navigation mesh plane
//!    at or below its centre, under the half-extent and the full-extent
//!    readings of the bound words (the data's own check of
//!    `world_trigger_box_v1`);
//! 4. the exterior grid: placed references of exterior cells whose
//!    position falls in their cell's grid square of [`CELL_SIZE`] units;
//! 5. the compiled scripts: handlers of the engine events the model
//!    raises, call sites of the distance registration natives (with their
//!    thresholds) and of position, cell, location and line-of-sight
//!    natives by name, and which of them the model answers.
use fo4_core::{Budget, Manifest};
use fo4_nav::{path::height_in, NavmeshRecord};
use fo4_plugin::{
    typed::{CellKind, Refr, Store},
    vmad::Vmad,
    FormKey, Record,
};
use fo4_vfs::Vfs;
use fo4_vm::{
    track::{primitive_of, CELL_SIZE, KIND_BOX, KIND_SPHERE},
    Lit, MapLibrary,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

/// Engine events the model raises or could raise.
const HANDLERS: [&str; 6] = [
    "ontriggerenter",
    "ontriggerleave",
    "ondistancelessthan",
    "ondistancegreaterthan",
    "ongainlos",
    "onlostlos",
];

/// Natives by call name, with whether the model answers them.
const NATIVES: [(&str, &str); 28] = [
    (
        "registerfordistancelessthanevent",
        "raised (--distance-events)",
    ),
    (
        "registerfordistancegreaterthanevent",
        "raised (--distance-events)",
    ),
    (
        "unregisterfordistanceevents",
        "answered (--distance-events)",
    ),
    ("getdistance", "answered (--position-natives)"),
    ("getpositionx", "answered (--position-natives)"),
    ("getpositiony", "answered (--position-natives)"),
    ("getpositionz", "answered (--position-natives)"),
    ("getanglez", "answered (--position-natives)"),
    ("getanglex", "not modelled"),
    ("getangley", "not modelled"),
    ("getheadingangle", "not modelled"),
    ("getparentcell", "answered (--cell-natives)"),
    ("getworldspace", "answered (--cell-natives)"),
    ("isinterior", "answered (--cell-natives)"),
    ("isininterior", "a scripted function"),
    ("getcurrentlocation", "answered (--location-native)"),
    ("isinlocation", "a scripted function"),
    ("hasdirectlos", "a caller assumption (--assume-los)"),
    ("hasdetectionlos", "not modelled"),
    ("registerfordirectlosgain", "not modelled"),
    ("registerfordirectloslost", "not modelled"),
    ("registerfordetectionlosgain", "not modelled"),
    ("registerfordetectionloslost", "not modelled"),
    (
        "moveto",
        "observed (the reference is no longer at its stored place)",
    ),
    (
        "setposition",
        "observed (the reference is no longer at its stored place)",
    ),
    (
        "movetonode",
        "observed (the reference is no longer at its stored place)",
    ),
    (
        "translateto",
        "observed (the reference is no longer at its stored place)",
    ),
    ("findclosestreferenceoftype", "not modelled"),
];

fn chain_declares(lib: &MapLibrary, script: &str, handler: &str) -> bool {
    let mut t = script.to_string();
    for _ in 0..fo4_vm::MAX_PARENTS {
        let Some(s) = lib.scripts.get(&t) else {
            return false;
        };
        if s.functions
            .iter()
            .any(|((_, n), f)| n == handler && !f.native() && !f.code.is_empty())
        {
            return true;
        }
        t = s.parent.clone();
        if t.is_empty() {
            return false;
        }
    }
    false
}

fn vmad_scripts(r: &Record, b: &Budget) -> Vec<String> {
    let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"VMAD") else {
        return Vec::new();
    };
    match Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, b) {
        Ok(v) => v
            .scripts
            .iter()
            .map(|x| fo4_pex::index::lower(x.name))
            .collect(),
        Err(_) => vec!["?unreadable".into()],
    }
}

fn bucket(d: f64) -> &'static str {
    if !d.is_finite() {
        "no plane at or below the centre"
    } else if d.abs() <= 1.0 {
        "within 1 unit of the plane"
    } else if d < 0.0 {
        "below the plane (the box reaches into the ground)"
    } else if d <= 16.0 {
        "1 to 16 units above the plane"
    } else if d <= 64.0 {
        "16 to 64 units above the plane"
    } else {
        "more than 64 units above the plane"
    }
}

fn threshold(l: Option<&Lit>) -> String {
    match l {
        Some(Lit::Float(f)) => {
            let f = f64::from(*f);
            let b = if f < 100.0 {
                "under 100"
            } else if f < 500.0 {
                "100 to under 500"
            } else if f < 2000.0 {
                "500 to under 2000"
            } else {
                "2000 or more"
            };
            format!("a literal {b}")
        }
        Some(Lit::Int(_)) => "an integer literal".into(),
        Some(Lit::Ident(_)) => "a variable".into(),
        _ => "other".into(),
    }
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("world_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let lib = fo4_vm::setters::read_all_scripts(&vfs, m.limits.max_memory)?;
    let max = m.limits.max_memory;
    let mut c = Counts::new();
    let mut names = String::new();
    // Boxes to check against the ground, by cell: (key, centre, half z
    // reading's half height).
    let mut ground_checks: BTreeMap<FormKey, Vec<(FormKey, [f64; 3], f64)>> = BTreeMap::new();
    for (r, cell, group) in store.placement_groups() {
        if r.signature != *b"REFR" || r.deleted {
            continue;
        }
        let k = store.overrides().form_key(r.key, store.order());
        let b = Budget::new(max);
        let Some(raw) = store.raw(&k, &b)? else {
            continue;
        };
        let exterior = store.kind(&cell) == Some(CellKind::Exterior);
        // 4: the exterior grid.
        if exterior && group == 9 {
            let refr = store.get::<Refr>(&k, &b)?;
            let grid = store
                .get::<fo4_plugin::typed::Cell>(&cell, &b)?
                .and_then(|x| x.data.grid);
            match (refr.and_then(|x| x.data.transform), grid) {
                (Some(t), Some(g)) => {
                    let gi = (f64::from(t[0]) / CELL_SIZE).floor() as i64;
                    let gj = (f64::from(t[1]) / CELL_SIZE).floor() as i64;
                    bump(
                        &mut c,
                        if (gi, gj) == (i64::from(g.i), i64::from(g.j)) {
                            "4 placed references of exterior cells (temporary children)\tposition in the cell's grid square"
                        } else {
                            "4 placed references of exterior cells (temporary children)\tposition outside the cell's grid square"
                        },
                    );
                }
                _ => bump(
                    &mut c,
                    "4 placed references of exterior cells (temporary children)\tno position or no grid",
                ),
            }
        }
        let n = raw
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"XPRM")
            .count();
        if n == 0 {
            continue;
        }
        // 1: the layout.
        let p = match primitive_of(&raw) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(why) => {
                bump(&mut c, format!("1 XPRM not read\t{why}"));
                continue;
            }
        };
        bump(&mut c, "1 placed references with an XPRM of 32 bytes");
        let base = fo4_vm::bind::base_of(&store, &k, &b)?;
        let sig = base
            .as_ref()
            .and_then(|x| store.signature(x))
            .map(|s| String::from_utf8_lossy(&s).to_string())
            .unwrap_or_else(|| "none".into());
        bump(
            &mut c,
            format!("1 by kind word and base record type\t{}\t{sig}", p.kind),
        );
        let bd = p.bounds;
        let pattern = if bd.iter().any(|x| !x.is_finite() || *x < 0.0) {
            "a bound not finite or negative"
        } else if bd[0] == bd[1] && bd[1] == bd[2] {
            "three equal bounds"
        } else if bd[1] == 0.0 && bd[2] == 0.0 {
            "only the first bound non-zero"
        } else if bd.contains(&0.0) {
            "a zero bound among others"
        } else {
            "three positive bounds, not all equal"
        };
        bump(
            &mut c,
            format!("1 bound words by kind\t{}\t{pattern}", p.kind),
        );
        bump(
            &mut c,
            format!(
                "1 colour words by kind\t{}\t{}",
                p.kind,
                if p.color.iter().all(|x| (0.0..=1.0).contains(x)) {
                    "three in 0 to 1"
                } else {
                    "outside 0 to 1"
                }
            ),
        );
        bump(
            &mut c,
            format!(
                "1 word 6 by kind\t{}\t{}",
                p.kind,
                if (0.0..=1.0).contains(&p.word6) {
                    "in 0 to 1"
                } else {
                    "outside 0 to 1"
                }
            ),
        );
        if sig != "ACTI" {
            continue;
        }
        // 2: the activators.
        let mut scripts = vmad_scripts(&raw, &b);
        if let Some(bk) = &base {
            if let Some(br) = store.raw(bk, &b)? {
                scripts.extend(vmad_scripts(&br, &b));
            }
        }
        let enter = scripts
            .iter()
            .any(|s| chain_declares(&lib, s, "ontriggerenter"));
        let leave = scripts
            .iter()
            .any(|s| chain_declares(&lib, s, "ontriggerleave"));
        let handler = match (enter, leave) {
            (true, true) => "enter and leave handlers",
            (true, false) => "an enter handler",
            (false, true) => "a leave handler",
            (false, false) if scripts.is_empty() => "no script",
            (false, false) => "scripts without a trigger handler",
        };
        let kind = match p.kind {
            KIND_BOX => "kind 1 (box: --trigger-box)",
            KIND_SPHERE => "kind 2 (sphere: --trigger-sphere)",
            _ => "another kind (not modelled)",
        };
        bump(
            &mut c,
            format!("2 placed activators with a primitive\t{kind}\t{handler}"),
        );
        if r.disabled {
            bump(
                &mut c,
                format!("2 placed activators with a primitive\t{kind}\tinitially disabled"),
            );
        }
        if raw.subrecords.iter().any(|s| s.signature == *b"XESP") {
            bump(
                &mut c,
                format!("2 placed activators with a primitive\t{kind}\twith an enable parent"),
            );
        }
        if group == 8 {
            bump(
                &mut c,
                format!("2 placed activators with a primitive\t{kind}\tpersistent"),
            );
        }
        bump(
            &mut c,
            format!(
                "2 placed activators with a primitive\t{kind}\tin an {}",
                if exterior {
                    "exterior cell"
                } else {
                    "interior cell"
                }
            ),
        );
        let refr = store.get::<Refr>(&k, &b)?;
        let (t, scale) = match refr {
            Some(x) => (x.data.transform, x.data.scale),
            None => (None, None),
        };
        if scale.is_some_and(|s| s != 1.0) {
            bump(
                &mut c,
                format!("2 placed activators with a primitive\t{kind}\ta scale other than 1"),
            );
        }
        let Some(t) = t else {
            bump(
                &mut c,
                format!("2 placed activators with a primitive\t{kind}\tno placement"),
            );
            continue;
        };
        let upright = t[3] == 0.0 && t[4] == 0.0;
        if !upright {
            bump(&mut c, format!("2 placed activators with a primitive\t{kind}\ttilted (a first or second rotation word not zero)"));
        }
        if p.kind == KIND_BOX && (enter || leave) && upright && scale.is_none_or(|s| s == 1.0) {
            ground_checks.entry(cell.clone()).or_default().push((
                k.clone(),
                [t[0], t[1], t[2]].map(f64::from),
                f64::from(p.bounds[2]),
            ));
        }
        let _ = writeln!(
            names,
            "{}\t{kind}\t{handler}\t{}",
            fo4_vm::bind::record_key(&k),
            scripts.join(",")
        );
    }
    // 3: boxes against the ground.
    for (cell, boxes) in &ground_checks {
        let b = Budget::new(max);
        let mut meshes = Vec::new();
        for kid in store
            .children(cell, &b)?
            .excluded
            .iter()
            .filter(|k| k.signature == *b"NAVM")
        {
            let rb = b.child();
            if let Some(r) = store.raw(&kid.key, &rb)? {
                if let Ok(n) = NavmeshRecord::decode(&r, &rb) {
                    meshes.push(n.mesh);
                }
            }
        }
        for (_, centre, hz) in boxes {
            let mut best = f64::NEG_INFINITY;
            for mesh in &meshes {
                for i in 0..mesh.triangles.len() {
                    let Some(corners) = mesh.corners(i) else {
                        continue;
                    };
                    if let Some(h) = height_in(&corners, centre[0], centre[1]) {
                        if h <= centre[2] && h > best {
                            best = h;
                        }
                    }
                }
            }
            for (reading, half) in [("half extents", *hz), ("full extents", hz / 2.0)] {
                let d = centre[2] - half - best;
                bump(
                    &mut c,
                    format!(
                        "3 upright boxes of activators with a trigger handler (unscaled), bottom against the highest mesh plane at or below the centre in the box's cell\tbound words as {reading}\t{}",
                        bucket(d)
                    ),
                );
            }
        }
    }
    // 5: the scripts.
    *c.entry("5 scripts read".into()).or_default() += lib.scripts.len() as u64;
    let names_by: BTreeMap<&str, &str> = NATIVES.iter().copied().collect();
    let mut declared: BTreeSet<(String, String)> = BTreeSet::new();
    for s in lib.scripts.values() {
        let mut fs: Vec<_> = s.functions.values().cloned().collect();
        for p in &s.properties {
            fs.extend(p.read.iter().cloned());
            fs.extend(p.write.iter().cloned());
        }
        for f in &fs {
            if HANDLERS.contains(&f.name.as_str()) && !f.native() && !f.code.is_empty() {
                declared.insert((f.name.clone(), s.name.clone()));
            }
            for i in &f.code {
                let name = match (i.op, i.args.first(), i.args.get(1)) {
                    (23 | 24, Some(Lit::Ident(n)), _) => n.as_str(),
                    (25, Some(Lit::Ident(_)), Some(Lit::Ident(n))) => n.as_str(),
                    _ => continue,
                };
                let Some(what) = names_by.get(name) else {
                    continue;
                };
                bump(&mut c, format!("5 call sites by name\t{name}\t{what}"));
                if name.starts_with("registerfordistance") {
                    bump(
                        &mut c,
                        format!(
                            "5 distance registrations by threshold argument\t{}",
                            threshold(i.extra.get(2))
                        ),
                    );
                    let _ = writeln!(names, "## {name} in {}.{}", s.name, f.name);
                }
            }
        }
    }
    for (h, s) in &declared {
        bump(
            &mut c,
            format!("5 scripts declaring a handler with code\t{h}"),
        );
        if h.starts_with("ondistance") {
            let _ = writeln!(names, "## handler {h} in {s}");
        }
    }
    let mut out = String::from(
        "# world_census: kinds and counts only (spec/vm.md, Player position). Sections: 1 the XPRM of every winning placed reference; 2 placed activators with a primitive;\n# 3 boxes against the navigation mesh under the two readings of the bound words; 4 the exterior grid of 4096 units; 5 compiled scripts: handlers, distance registrations and position natives by call name.\n",
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
