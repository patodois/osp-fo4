#![forbid(unsafe_code)]
//! Census of what the data stores that bears on light for the cells a run
//! timeline's player passes through (`spec/light.md`, `spec/light-census.txt`):
//! interior cells' lighting fields and their template under the `Inherits`
//! word, placed lights (radius, colour, type declarations, fade value,
//! reference subrecords), the exterior's worldspace climate and its
//! weathers (colour table by row and slot, directional ambient, fog
//! values, image spaces), the regions the exterior cells name, and
//! optionally a global's stored value (the caller names it). Stored
//! numbers and counts only in `OUT.txt`; record keys and editor ids only in
//! `NAMES.txt` (never versioned).
//!
//! `light_census MANIFEST TIMELINE OUT.txt NAMES.txt [--global EDITOR_ID]`
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    layout,
    typed::{Cell, CellKind, Ligh, Refr, Store, Wrld},
    weather::{Climate, Weather},
    FormKey,
};
use fo4_scene::{
    light::{cell_values, CellValues},
    timeline::{Timeline, What},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;

fn key(k: &FormKey) -> String {
    format!("{}:{:08X}", k.plugin, k.object_id)
}
fn c3(c: Option<[u8; 4]>) -> String {
    c.map_or("-".into(), |c| format!("{},{},{}", c[0], c[1], c[2]))
}
fn f(v: Option<f32>) -> String {
    v.map_or("-".into(), |v| format!("{v}"))
}
fn values(out: &mut String, what: &str, v: &CellValues) {
    let _ = writeln!(
        out,
        "  {what}: ambient {} directional {} rotation {} {} fade {} fog near colour {} far colour {} near {} far {} power {} max {} clip {}",
        c3(v.ambient),
        c3(v.directional),
        v.rotation[0].map_or("-".into(), |x| x.to_string()),
        v.rotation[1].map_or("-".into(), |x| x.to_string()),
        f(v.fade),
        c3(v.fog_near_colour),
        c3(v.fog_far_colour),
        f(v.fog_near),
        f(v.fog_far),
        f(v.fog_power),
        f(v.fog_max),
        f(v.clip)
    );
    let _ = writeln!(
        out,
        "  {what} directional ambient X+ X- Y+ Y- Z+ Z-: {} scale {}",
        v.cube.iter().map(|c| c3(*c)).collect::<Vec<_>>().join(" "),
        f(v.cube_scale)
    );
}
fn stats(v: &mut [f64]) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.sort_by(f64::total_cmp);
    format!(
        "min {} median {} max {} (n {})",
        v[0],
        v[v.len() / 2],
        v[v.len() - 1],
        v.len()
    )
}

#[derive(Default)]
struct Lights {
    active: u64,
    disabled: u64,
    radius: Vec<f64>,
    fade: Vec<f64>,
    colours: BTreeSet<[u8; 3]>,
    white: u64,
    types: BTreeMap<String, u64>,
    flag_words: BTreeMap<u32, u64>,
    reference_tags: BTreeMap<String, u64>,
    no_transform: u64,
    scaled: u64,
    /// XRDS against the base radius: below, equal, above; the ratios.
    xrds: [u64; 3],
    xrds_ratio: Vec<f64>,
    xlig_sizes: BTreeMap<usize, u64>,
}
impl Lights {
    fn add(&mut self, store: &Store, child: &FormKey, active: bool, b: &Budget) -> Result<(), E> {
        let Some(r) = store.get::<Refr>(child, b)? else {
            return Ok(());
        };
        let Some(base) = r.data.base.as_ref().and_then(|v| v.key.clone()) else {
            return Ok(());
        };
        if store.signature(&base) != Some(*b"LIGH") {
            return Ok(());
        }
        let Some(l) = store.get::<Ligh>(&base, b)? else {
            return Ok(());
        };
        if active {
            self.active += 1;
        } else {
            self.disabled += 1;
        }
        if r.data.transform.is_none() {
            self.no_transform += 1;
        }
        if r.data.scale.is_some_and(|s| s != 1.) {
            self.scaled += 1;
        }
        let xrds = r.data.common.layout_fields.iter().find_map(|f| {
            match (
                r.record.subrecords[f.subrecord].signature == *b"XRDS",
                &f.value,
            ) {
                (true, layout::Value::Float(v)) => Some(f64::from(*v)),
                _ => None,
            }
        });
        if let (Some(x), Some(d)) = (xrds, &l.data.light) {
            let base = f64::from(d.radius);
            self.xrds[if x < base {
                0
            } else if x == base {
                1
            } else {
                2
            }] += 1;
            if base > 0. {
                self.xrds_ratio.push(x / base);
            }
        }
        for s in &r.record.subrecords {
            if s.signature == *b"XLIG" {
                *self.xlig_sizes.entry(r.record.data(s).len()).or_default() += 1;
            }
            *self
                .reference_tags
                .entry(String::from_utf8_lossy(&s.signature).into_owned())
                .or_default() += 1;
        }
        if let Some(d) = &l.data.light {
            self.radius.push(f64::from(d.radius));
            self.colours.insert([d.color[0], d.color[1], d.color[2]]);
            if d.color[..3] == [255, 255, 255] {
                self.white += 1;
            }
            *self.flag_words.entry(d.flags).or_default() += 1;
        }
        for s in &l.record.subrecords {
            if s.signature == *b"FNAM" {
                if let Ok(w) = <[u8; 4]>::try_from(l.record.data(s)) {
                    self.fade.push(f64::from(f32::from_le_bytes(w)));
                }
            }
        }
        let mut declared = false;
        for field in &l.data.common.layout_fields {
            if l.record.subrecords[field.subrecord].signature != *b"DATA" {
                continue;
            }
            if let layout::Value::Flags { printed_labels, .. } = field.value {
                for name in printed_labels
                    .unwrap_or("")
                    .split(", ")
                    .filter(|n| !n.is_empty())
                {
                    declared = true;
                    *self.types.entry(name.into()).or_default() += 1;
                }
            }
        }
        if !declared {
            *self.types.entry("(no printed label)".into()).or_default() += 1;
        }
        Ok(())
    }
    fn write(&mut self, out: &mut String) {
        let _ = writeln!(
            out,
            "  placed lights: {} active, {} initially disabled; {} without a placement, {} with a scale other than 1",
            self.active, self.disabled, self.no_transform, self.scaled
        );
        let _ = writeln!(out, "  radius: {}", stats(&mut self.radius));
        let _ = writeln!(out, "  fade value (FNAM): {}", stats(&mut self.fade));
        let _ = writeln!(
            out,
            "  reference radius (XRDS) against the base radius: {} below, {} equal, {} above; ratio {}",
            self.xrds[0],
            self.xrds[1],
            self.xrds[2],
            stats(&mut self.xrds_ratio)
        );
        let _ = writeln!(
            out,
            "  XLIG sizes: {}",
            self.xlig_sizes
                .iter()
                .map(|(s, n)| format!("{s} bytes {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(
            out,
            "  colours: {} distinct, {} lights white (255,255,255)",
            self.colours.len(),
            self.white
        );
        for (t, n) in &self.types {
            let _ = writeln!(out, "  flags label {t}: {n}");
        }
        for (w, n) in &self.flag_words {
            let _ = writeln!(out, "  flags word {w:#010x}: {n}");
        }
        let tags: Vec<String> = self
            .reference_tags
            .iter()
            .map(|(t, n)| format!("{t} {n}"))
            .collect();
        let _ = writeln!(out, "  reference subrecords: {}", tags.join(", "));
    }
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str = "light_census MANIFEST TIMELINE OUT.txt NAMES.txt [--global EDITOR_ID]";
    if args.len() != 4 && args.len() != 6 {
        return Err(USAGE.into());
    }
    let global = if args.len() == 6 {
        if args[4] != "--global" {
            return Err(USAGE.into());
        }
        Some(args[5].clone())
    } else {
        None
    };
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let t = Timeline::parse(&std::fs::read_to_string(&args[1])?)?;
    let mut out = String::new();
    let mut names = String::new();
    let _ = writeln!(
        out,
        "# What the data stores that bears on light along a run timeline's player cells (light_census; stored numbers and counts only)"
    );
    // The player's cells in first-visit order, with the seconds spent.
    let mut cells: Vec<FormKey> = Vec::new();
    let mut first_second: BTreeMap<FormKey, f64> = BTreeMap::new();
    for e in &t.events {
        if let What::Player { cell: Some(c), .. } = &e.what {
            if !cells.contains(c) {
                cells.push(c.clone());
                first_second.insert(c.clone(), e.t);
            }
        }
    }
    let kinds: Vec<Option<CellKind>> = cells.iter().map(|c| store.kind(c)).collect();
    let interior: Vec<&FormKey> = cells
        .iter()
        .zip(&kinds)
        .filter(|(_, k)| **k == Some(CellKind::Interior))
        .map(|(c, _)| c)
        .collect();
    let exterior: Vec<&FormKey> = cells
        .iter()
        .zip(&kinds)
        .filter(|(_, k)| **k == Some(CellKind::Exterior))
        .map(|(c, _)| c)
        .collect();
    let _ = writeln!(
        out,
        "player cells: {} distinct, {} interior, {} exterior",
        cells.len(),
        interior.len(),
        exterior.len()
    );

    let _ = writeln!(out, "\n## Interior cells (first-visit order)");
    for (k, c) in interior.iter().enumerate() {
        let _ = writeln!(names, "interior {}\t{}", k + 1, key(c));
        let cell = store.get::<Cell>(c, &b)?.ok_or("no interior cell record")?;
        let _ = writeln!(
            out,
            "interior {} (from second {}):",
            k + 1,
            first_second[*c]
        );
        let xcll = cell
            .record
            .subrecords
            .iter()
            .find(|s| s.signature == *b"XCLL")
            .map(|s| cell.record.data(s).len());
        let _ = writeln!(
            out,
            "  XCLL {}; lighting template {}; image space {}",
            xcll.map_or("absent".into(), |n| format!("{n} bytes")),
            if cell
                .data
                .lighting_template
                .as_ref()
                .and_then(|r| r.key.as_ref())
                .is_some()
            {
                "named"
            } else {
                "none"
            },
            if cell.data.image_space.is_some() {
                "named"
            } else {
                "none"
            }
        );
        if let Some(v) = cell_values(
            &cell.record,
            &cell.data.common.layout_fields,
            b"XCLL",
            b"XCLL",
        ) {
            values(&mut out, "cell", &v);
            if let Some((raw, labels)) = v.inherits {
                let _ = writeln!(
                    out,
                    "  inherits word {raw:#010x}: {}",
                    labels.unwrap_or("(no fully named labels)")
                );
            }
        }
        if let Some(tk) = cell
            .data
            .lighting_template
            .as_ref()
            .and_then(|r| r.key.clone())
        {
            let _ = writeln!(names, "interior {} template\t{}", k + 1, key(&tk));
            if let Some(tpl) = store.get::<fo4_plugin::typed::Lgtm>(&tk, &b)? {
                if let Some(v) = cell_values(
                    &tpl.record,
                    &tpl.data.common.layout_fields,
                    b"DATA",
                    b"DALC",
                ) {
                    values(&mut out, "template", &v);
                }
            }
        }
        if let Some(ik) = cell.data.image_space.as_ref().and_then(|r| r.key.clone()) {
            let _ = writeln!(names, "interior {} image space\t{}", k + 1, key(&ik));
            if let Some(r) = store.raw(&ik, &b)? {
                let tags: Vec<String> = r
                    .subrecords
                    .iter()
                    .map(|s| {
                        format!(
                            "{} {}",
                            String::from_utf8_lossy(&s.signature),
                            r.data(s).len()
                        )
                    })
                    .collect();
                let _ = writeln!(
                    out,
                    "  image space subrecords (tag bytes): {}",
                    tags.join(", ")
                );
            }
        }
        let ch = store.children(c, &b)?;
        let mut lights = Lights::default();
        for x in ch.active.iter().filter(|x| x.signature == *b"REFR") {
            lights.add(&store, &x.key, true, &b)?;
        }
        for x in ch.disabled.iter().filter(|x| x.signature == *b"REFR") {
            lights.add(&store, &x.key, false, &b)?;
        }
        lights.write(&mut out);
    }

    let _ = writeln!(out, "\n## Exterior cells");
    let mut worlds: BTreeSet<FormKey> = BTreeSet::new();
    let mut around: BTreeSet<FormKey> = BTreeSet::new();
    let mut regions: BTreeMap<FormKey, u64> = BTreeMap::new();
    let mut weather_from_region = 0u64;
    let mut cell_lighting = 0u64;
    let mut cell_template = 0u64;
    for c in &exterior {
        if let Some(w) = store.world(c) {
            worlds.insert(w);
        }
        let a = fo4_scene::grid::around(&store, c, 1, &b)?;
        around.insert((*c).clone());
        around.extend(a.neighbours.into_iter().filter_map(|n| n.cell));
    }
    let mut lights = Lights::default();
    for c in &around {
        let cell = store.get::<Cell>(c, &b)?.ok_or("no exterior cell record")?;
        for s in &cell.record.subrecords {
            let d = cell.record.data(s);
            match &s.signature {
                b"XCLR" => {
                    for w in d.as_chunks::<4>().0 {
                        let raw = u32::from_le_bytes(*w);
                        let plugin = store.overrides().winner(c).map(|w| w.plugin).unwrap_or(0);
                        if let Some(k) =
                            store
                                .order()
                                .remap(plugin, raw, &cell.record.source.file, s.offset)?
                        {
                            *regions.entry(k).or_default() += 1;
                        }
                    }
                }
                b"XCCM" => weather_from_region += 1,
                b"XCLL" => cell_lighting += 1,
                _ => {}
            }
        }
        if cell
            .data
            .lighting_template
            .as_ref()
            .and_then(|r| r.key.as_ref())
            .is_some()
        {
            cell_template += 1;
        }
        let ch = store.children(c, &b)?;
        for x in ch.active.iter().filter(|x| x.signature == *b"REFR") {
            lights.add(&store, &x.key, true, &b)?;
        }
        for x in ch.disabled.iter().filter(|x| x.signature == *b"REFR") {
            lights.add(&store, &x.key, false, &b)?;
        }
    }
    let _ = writeln!(
        out,
        "player exterior cells {} and their neighbours within one grid step: {} cells; with XCLL {}, with a lighting template {}, with XCCM {}",
        exterior.len(),
        around.len(),
        cell_lighting,
        cell_template,
        weather_from_region
    );
    lights.write(&mut out);
    let _ = writeln!(
        out,
        "regions named by those cells (XCLR): {}",
        regions.len()
    );
    for (k, (r, n)) in regions.iter().enumerate() {
        let _ = writeln!(names, "region {}\t{}", k + 1, key(r));
        let Some(rec) = store.raw(r, &b)? else {
            continue;
        };
        let mut tags: Vec<String> = Vec::new();
        let mut rdat: Vec<String> = Vec::new();
        for s in &rec.subrecords {
            let d = rec.data(s);
            tags.push(format!(
                "{} {}",
                String::from_utf8_lossy(&s.signature),
                d.len()
            ));
            if s.signature == *b"RDAT" && d.len() >= 4 {
                rdat.push(u32::from_le_bytes([d[0], d[1], d[2], d[3]]).to_string());
            }
        }
        let _ = writeln!(
            out,
            "  region {} (named by {n} cells): subrecords (tag bytes) {}; RDAT first words {}",
            k + 1,
            tags.into_iter()
                .filter(|t| !t.starts_with("EDID"))
                .collect::<Vec<_>>()
                .join(", "),
            if rdat.is_empty() {
                "none".into()
            } else {
                rdat.join(" ")
            }
        );
    }

    let _ = writeln!(out, "\n## Worldspaces, climates and weathers");
    for (k, w) in worlds.iter().enumerate() {
        let _ = writeln!(names, "world {}\t{}", k + 1, key(w));
        let wr = store.get::<Wrld>(w, &b)?.ok_or("no world record")?;
        let _ = writeln!(
            out,
            "world {}: climate {}; lighting template {}; parent {}",
            k + 1,
            if wr.data.climate.is_some() {
                "named"
            } else {
                "none"
            },
            if wr.data.lighting_template.is_some() {
                "named"
            } else {
                "none"
            },
            if wr.data.parent.is_some() {
                "named"
            } else {
                "none"
            }
        );
        let Some(ck) = wr.data.climate.as_ref().and_then(|r| r.key.clone()) else {
            continue;
        };
        let _ = writeln!(names, "world {} climate\t{}", k + 1, key(&ck));
        let (cr, cp) = store.raw_with_plugin(&ck, &b)?.ok_or("no climate record")?;
        let climate = Climate::read(
            &cr,
            Context {
                order: store.order(),
                plugin: cp,
            },
            &b,
        )?;
        let _ = writeln!(
            out,
            "  climate: {} weather list entries; times (TNAM bytes) {}",
            climate.weathers.len(),
            climate.times.map_or("-".into(), |t| format!(
                "{:?}; the first four as tens of minutes: {}",
                t,
                t[..4]
                    .iter()
                    .map(|v| format!("{}:{:02}", u32::from(*v) * 10 / 60, u32::from(*v) * 10 % 60))
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
        );
        for (j, e) in climate.weathers.iter().enumerate() {
            let target = e.weather.key.as_ref().and_then(|k| store.signature(k));
            let _ = writeln!(
                out,
                "  entry {}: first word resolves to {}, second word {}, third word {}",
                j + 1,
                target.map_or("nothing".into(), |s| String::from_utf8_lossy(&s)
                    .into_owned()),
                e.chance,
                e.global
                    .as_ref()
                    .and_then(|g| g.key.as_ref())
                    .and_then(|k| store.signature(k))
                    .map_or("zero".into(), |s| String::from_utf8_lossy(&s).into_owned())
            );
            let Some(wk) = e.weather.key.clone() else {
                continue;
            };
            let _ = writeln!(names, "world {} weather {}\t{}", k + 1, j + 1, key(&wk));
            let (r, p) = store.raw_with_plugin(&wk, &b)?.ok_or("no weather record")?;
            let wt = Weather::read(
                &r,
                Context {
                    order: store.order(),
                    plugin: p,
                },
                &b,
            )?;
            let mut tags: BTreeMap<String, (u64, BTreeSet<usize>)> = BTreeMap::new();
            for s in &r.subrecords {
                let e = tags
                    .entry(String::from_utf8_lossy(&s.signature).into_owned())
                    .or_default();
                e.0 += 1;
                e.1.insert(r.data(s).len());
            }
            let _ = writeln!(
                out,
                "  weather {}: subrecords (tag count sizes): {}",
                j + 1,
                tags.iter()
                    .filter(|(t, _)| t.as_str() != "EDID")
                    .map(|(t, (n, s))| format!(
                        "{t} x{n} {}",
                        s.iter()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join("/")
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let _ = writeln!(
                out,
                "  weather {}: colour table {} rows by {} slots (row: R,G,B per slot)",
                j + 1,
                wt.rows(),
                wt.slots()
            );
            for row in 0..wt.rows() {
                let cells: Vec<String> = (0..wt.slots()).map(|s| c3(wt.colour(row, s))).collect();
                let fourth: BTreeSet<u8> = (0..wt.slots())
                    .filter_map(|s| wt.colour(row, s))
                    .map(|c| c[3])
                    .collect();
                let _ = writeln!(
                    out,
                    "    row {row:2}: {} (fourth bytes {:?})",
                    cells.join(" "),
                    fourth
                );
            }
            for (s, d) in wt.ambient.iter().enumerate() {
                let _ =
                    writeln!(
                    out,
                    "    directional ambient slot {s}: X+ X- Y+ Y- Z+ Z- {} specular {} scale {}",
                    d.axes.iter().map(|c| c3(Some(*c))).collect::<Vec<_>>().join(" "),
                    c3(Some(d.specular)),
                    d.scale
                );
            }
            let _ = writeln!(
                out,
                "    FNAM values: {}",
                wt.fog
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let distinct: BTreeSet<Option<FormKey>> =
                wt.image_spaces.iter().map(|r| r.key.clone()).collect();
            let _ = writeln!(
                out,
                "    image spaces: {} entries, {} distinct",
                wt.image_spaces.len(),
                distinct.len()
            );
        }
    }

    let _ = writeln!(out, "\n## Every climate and weather of the load order");
    let mut clmt = 0u64;
    let mut wlst: BTreeMap<usize, u64> = BTreeMap::new();
    let mut wthr: BTreeMap<(usize, usize), u64> = BTreeMap::new();
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    let winners: Vec<(FormKey, [u8; 4])> = store
        .winners()
        .filter(|w| w.signature == *b"CLMT" || w.signature == *b"WTHR")
        .map(|w| {
            (
                store.overrides().form_key(w.key, store.order()),
                w.signature,
            )
        })
        .collect();
    for (k, sig) in winners {
        let Some((r, p)) = store.raw_with_plugin(&k, &b)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin: p,
        };
        if sig == *b"CLMT" {
            clmt += 1;
            match Climate::read(&r, ctx, &b) {
                Ok(c) => *wlst.entry(c.weathers.len()).or_default() += 1,
                Err(e) => *refused.entry(e.code.to_string()).or_default() += 1,
            }
        } else {
            match Weather::read(&r, ctx, &b) {
                Ok(w) => *wthr.entry((w.slots(), w.rows())).or_default() += 1,
                Err(e) => *refused.entry(e.code.to_string()).or_default() += 1,
            }
        }
    }
    let _ = writeln!(
        out,
        "climates: {clmt}; by weather list entries: {}",
        wlst.iter()
            .map(|(n, c)| format!("{n} entries {c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        out,
        "weathers by slots and rows: {}",
        wthr.iter()
            .map(|((s, r), c)| format!("{s} slots {r} rows {c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        out,
        "refused by the readers: {}",
        if refused.is_empty() {
            "none".into()
        } else {
            refused
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    );

    if let Some(g) = global {
        let _ = writeln!(out, "\n## The caller's global");
        let found = store
            .winners()
            .filter(|w| w.signature == *b"GLOB")
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect::<Vec<_>>();
        let mut value = None;
        for k in found {
            let Some(r) = store.raw(&k, &b)? else {
                continue;
            };
            let edid = r
                .subrecords
                .iter()
                .find(|s| s.signature == *b"EDID")
                .map(|s| r.data(s).to_vec());
            if edid.as_deref().map(|d| d.strip_suffix(&[0]).unwrap_or(d)) == Some(g.as_bytes()) {
                let _ = writeln!(names, "global\t{}", key(&k));
                value = r
                    .subrecords
                    .iter()
                    .find(|s| s.signature == *b"FLTV")
                    .and_then(|s| <[u8; 4]>::try_from(r.data(s)).ok())
                    .map(f32::from_le_bytes);
            }
        }
        let _ = writeln!(
            out,
            "the caller's global (named in the notes, not here): stored value {}",
            f(value)
        );
    }
    std::fs::write(&args[2], out)?;
    std::fs::write(&args[3], names)?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
