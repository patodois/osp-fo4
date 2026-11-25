#![forbid(unsafe_code)]
//! Census of the poses step (`spec/poses-census.txt`): why the characters
//! of a run timeline's frames keep the rest pose, why the markers left are
//! markers, what the skinned models of placed records other than actors
//! are, and what the shaft model leaves out. Counts, stored numbers and
//! descriptions only in `OUT.txt`; record keys, editor ids and file names
//! only in `NAMES.txt` (never versioned).
//!
//! `poses_census MANIFEST TIMELINE OUT.txt NAMES.txt [--seed N] [--frame S=OVERLAY]... [--models] [--shaft PLUGIN:HEX]`
//!
//! 1. every character a frame's overlay (`--frame`, the headless viewer's
//!    or the CPU render's `overlay.txt`) draws in the rest pose, resolved
//!    as the cast resolves it with every look reading on (seed `--seed`),
//!    classed by why its race's subgraph entries refuse it: the keywords
//!    the readings settle, the entries of each flag word, the nearest
//!    entry's missing keywords and where in the actor's records they are
//!    stored (its chain of NPC_ and leveled lists, its outfit's armours,
//!    its inventory's items);
//! 2. every marker of those frames by its named reason, the controller
//!    blocks that keep an actor's model from the draw by family and by the
//!    part of the actor that holds them;
//! 3. `--models`: the models of placed records other than actors that hold
//!    skinned shapes, by base kind, by what they hold beside the skin and
//!    by the role their bones suggest (bone count, root names' kinds);
//!    the placements of the frames' cells that use them;
//! 4. `--shaft`: the named reference's model, block by block: what the
//!    draw leaves out and why.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::{Leveled, Npc, Otft},
    typed::Store,
    FormKey, Record,
};
use fo4_scene::{
    actor::{self, Readings},
    char_graph,
    timeline::{self as tl},
    Loader, RuleLevel,
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
fn edid(r: &Record) -> String {
    r.subrecords
        .iter()
        .find(|s| s.signature == *b"EDID")
        .map(|s| {
            let d = r.data(s);
            String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned()
        })
        .unwrap_or_default()
}
fn edid_of(store: &Store, k: &FormKey, b: &Budget) -> String {
    match store.raw(k, b) {
        Ok(Some(r)) => edid(&r),
        _ => String::new(),
    }
}

/// One frame's overlay lines: characters (key, rest-pose reason or empty)
/// and markers (key, reason).
struct Frame {
    second: String,
    characters: Vec<(FormKey, String)>,
    markers: Vec<(FormKey, String)>,
}
fn read_overlay(second: &str, path: &str) -> Result<Frame, E> {
    let text = std::fs::read_to_string(path)?;
    let mut f = Frame {
        second: second.to_string(),
        characters: Vec::new(),
        markers: Vec::new(),
    };
    for l in text.lines() {
        let cols: Vec<&str> = l.split('\t').collect();
        let Some(kind) = cols.first() else { continue };
        if *kind != "character" && *kind != "marker instead" {
            continue;
        }
        let Some(k) = cols.get(2).and_then(|s| tl::parse_key(s)) else {
            continue;
        };
        let rest = cols[3..].join("\t");
        if *kind == "character" {
            let why = rest
                .split("; ")
                .find(|p| p.starts_with("rest pose"))
                .unwrap_or("")
                .to_string();
            f.characters.push((tl::norm(&k), why));
        } else {
            f.markers
                .push((tl::norm(&k), cols.last().unwrap_or(&"").to_string()));
        }
    }
    Ok(f)
}

/// The names (lower case) of a model's sequences whose controlled blocks
/// name an effect-shader controller.
fn effect_sequences(n: &fo4_nif::Nif<'_>) -> BTreeSet<String> {
    let ty = |i: usize| {
        n.blocks
            .get(i)
            .and_then(|b| n.block_types.get(b.type_index as usize))
            .map(String::as_str)
            .unwrap_or("")
    };
    let mut out = BTreeSet::new();
    for (i, blk) in n.blocks.iter().enumerate() {
        if ty(i) != "NiControllerSequence" {
            continue;
        }
        let Some(d) = &blk.decoded else { continue };
        let fs = fo4_scene::model_policy::fields(d);
        let effect = fs.iter().any(|f| {
            f.name == "Controller"
                && matches!(f.value, fo4_nif::Value::Ref(r) if r >= 0 && ty(r as usize).starts_with("BSEffectShaderProperty"))
        });
        if !effect {
            continue;
        }
        if let Some(name) = fs
            .iter()
            .find(|f| f.name == "Name")
            .and_then(|f| match f.value {
                fo4_nif::Value::StringIndex(x) => n.strings.get(x as usize),
                _ => None,
            })
        {
            out.insert(String::from_utf8_lossy(name).to_ascii_lowercase());
        }
    }
    out
}

/// A path's bytes lower case with forward slashes.
fn norm_path(p: &[u8]) -> String {
    String::from_utf8_lossy(p)
        .to_ascii_lowercase()
        .replace('\\', "/")
}

/// A marker's reason with keys, numbers and paths replaced.
fn marker_class(why: &str) -> String {
    let mut out = String::new();
    for (i, w) in why.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let t = w.trim_end_matches([';', ',', ')', ':']);
        if t.contains(['/', '\\'])
            || t.contains(".nif")
            || t.contains(".dds")
            || (t.len() > 16 && t.chars().all(|c| c.is_ascii_hexdigit()))
        {
            out.push_str("(a path)");
        } else if t.split_once(':').is_some_and(|(p, h)| {
            !p.is_empty() && h.len() >= 5 && h.chars().all(|c| c.is_ascii_hexdigit())
        }) || (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
        {
            out.push('N');
        } else {
            out.push_str(w);
            continue;
        }
        out.push_str(&w[t.len()..]);
    }
    out.chars().take(160).collect()
}

/// The family of an open blocker: its rest tag (`rest=TAG`), else the
/// first word of what was found.
fn family(e: &fo4_core::Error) -> String {
    if let Some(t) = e.found.split(";rest=").nth(1) {
        return format!("rest {}", t.split([';', ' ']).next().unwrap_or(""));
    }
    let first = e.found.split([' ', ';']).next().unwrap_or("");
    if first.contains(['/', '\\'])
        || first.contains('.')
        || (first.len() > 16 && first.chars().all(|c| c.is_ascii_hexdigit()))
    {
        "(a path)".into()
    } else {
        first.to_string()
    }
}

/// The KWDA keywords of a record (remapped).
fn kwda(store: &Store, k: &FormKey, b: &Budget) -> Result<BTreeSet<FormKey>, E> {
    let Some((r, p)) = store.raw_with_plugin(k, b)? else {
        return Ok(BTreeSet::new());
    };
    Ok(char_graph::record_keywords(store, &r, p)?
        .into_iter()
        .collect())
}

/// Where an actor's records store keywords: its chain of NPC_ and leveled
/// lists (every template slot, every entry), its outfits' armours (through
/// leveled item lists) and its inventory items (CNTO, through leveled item
/// lists), each a set of (place, keyword).
fn keyword_places(
    store: &Store,
    base: &FormKey,
    b: &Budget,
) -> Result<BTreeMap<FormKey, BTreeSet<&'static str>>, E> {
    let mut out: BTreeMap<FormKey, BTreeSet<&'static str>> = BTreeMap::new();
    let mut add = |place: &'static str, ks: BTreeSet<FormKey>| {
        for k in ks {
            out.entry(k).or_default().insert(place);
        }
    };
    let mut stack: Vec<(FormKey, u32)> = vec![(base.clone(), 0)];
    let mut seen: BTreeSet<FormKey> = BTreeSet::new();
    let mut items: Vec<(FormKey, &'static str, u32)> = Vec::new();
    while let Some((k, d)) = stack.pop() {
        if d > 8 || !seen.insert(k.clone()) {
            continue;
        }
        let Some((r, p)) = store.raw_with_plugin(&k, b)? else {
            continue;
        };
        let ctx = fo4_plugin::actor::Context {
            order: store.order(),
            plugin: p,
        };
        match &r.signature {
            b"LVLN" => {
                if let Ok(l) = Leveled::decode(&r, ctx, b) {
                    stack.extend(
                        l.entries
                            .iter()
                            .filter_map(|e| e.reference.key.clone())
                            .map(|x| (x, d + 1)),
                    );
                }
            }
            b"NPC_" => {
                add("an NPC_ of its chain", kwda(store, &k, b)?);
                if let Ok(n) = Npc::decode(&r, ctx, b) {
                    if let Some(slots) = &n.template_slots {
                        stack.extend(
                            slots
                                .iter()
                                .flatten()
                                .filter_map(|f| f.key.clone())
                                .map(|x| (x, d + 1)),
                        );
                    }
                    stack.extend(n.template.and_then(|f| f.key).map(|x| (x, d + 1)));
                    if let Some(o) = n.outfit.and_then(|f| f.key) {
                        if let Some((or, op)) = store.raw_with_plugin(&o, b)? {
                            let octx = fo4_plugin::actor::Context {
                                order: store.order(),
                                plugin: op,
                            };
                            if let Ok(otft) = Otft::decode(&or, octx, b) {
                                items.extend(
                                    otft.items
                                        .iter()
                                        .filter_map(|f| f.key.clone())
                                        .map(|x| (x, "an armour of its outfit", 0)),
                                );
                            }
                        }
                    }
                }
                for s in r.subrecords.iter().filter(|s| s.signature == *b"CNTO") {
                    let dd = r.data(s);
                    if let Some(w) = dd.get(..4) {
                        let id = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
                        if let Some(x) = store.order().remap(p, id, &r.source.file, s.offset)? {
                            items.push((x, "an item of its inventory", 0));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut seen_items: BTreeSet<FormKey> = BTreeSet::new();
    let mut n = 0;
    while let Some((k, place, d)) = items.pop() {
        n += 1;
        if n > 4096 || d > 8 || !seen_items.insert(k.clone()) {
            continue;
        }
        let Some((r, p)) = store.raw_with_plugin(&k, b)? else {
            continue;
        };
        match &r.signature {
            b"LVLI" => {
                let ctx = fo4_plugin::actor::Context {
                    order: store.order(),
                    plugin: p,
                };
                if let Ok(l) = Leveled::decode(&r, ctx, b) {
                    items.extend(
                        l.entries
                            .iter()
                            .filter_map(|e| e.reference.key.clone())
                            .map(|x| (x, place, d + 1)),
                    );
                }
            }
            b"ARMO" | b"WEAP" => {
                let place = match (&r.signature, place) {
                    (b"WEAP", "an item of its inventory") => "a weapon of its inventory",
                    (b"ARMO", "an item of its inventory") => "an armour of its inventory",
                    _ => place,
                };
                add(place, kwda(store, &k, b)?);
            }
            _ => {}
        }
    }
    Ok(out)
}

fn ty<'n>(n: &'n fo4_nif::Nif<'_>, i: usize) -> &'n str {
    n.blocks
        .get(i)
        .and_then(|b| n.block_types.get(b.type_index as usize))
        .map(String::as_str)
        .unwrap_or("")
}
fn block_fields<'n>(n: &'n fo4_nif::Nif<'_>, i: usize) -> &'n [fo4_nif::Field] {
    n.blocks
        .get(i)
        .and_then(|b| b.decoded.as_ref())
        .map(fo4_scene::model_policy::fields)
        .unwrap_or(&[])
}
fn node_name(n: &fo4_nif::Nif<'_>, i: usize) -> Option<String> {
    block_fields(n, i)
        .iter()
        .find(|f| f.name == "Name")
        .and_then(|f| match f.value {
            fo4_nif::Value::StringIndex(x) => n.strings.get(x as usize),
            _ => None,
        })
        .map(|s| String::from_utf8_lossy(s).into_owned())
}
fn links_of(n: &fo4_nif::Nif<'_>, i: usize, name: &str) -> Vec<usize> {
    block_fields(n, i)
        .iter()
        .filter(|f| f.name == name)
        .filter_map(|f| match f.value {
            fo4_nif::Value::Ref(r) if r >= 0 => Some(r as usize),
            _ => None,
        })
        .collect()
}
fn bucket(x: usize) -> &'static str {
    match x {
        0 => "0",
        1 => "1",
        2..=5 => "2 to 5",
        6..=20 => "6 to 20",
        _ => "more than 20",
    }
}

/// What moves a skinned model's bones, read from the model alone: the
/// sequences of its controller manager that name a bone, its behaviour
/// graph, physics constraints; and its size.
fn skin_role(
    n: &fo4_nif::Nif<'_>,
    file: &str,
    m: &Manifest,
    b: &Budget,
) -> Result<(String, String), E> {
    let mut bones: BTreeSet<usize> = BTreeSet::new();
    let mut shapes = 0usize;
    for i in 0..n.blocks.len() {
        if !matches!(n.blocks[i].decoded, Some(fo4_nif::Data::Mesh(_))) {
            continue;
        }
        if let Ok(Some(sk)) = fo4_nif::skin::shape_skin(n, i, file, &m.limits, b) {
            shapes += 1;
            bones.extend(sk.bones.iter().map(|x| x.node));
        }
    }
    let names: BTreeSet<String> = bones.iter().filter_map(|i| node_name(n, *i)).collect();
    let seqs = fo4_scene::sequence::sequences(n);
    let moving = seqs
        .iter()
        .filter(|s| s.blocks.iter().any(|(x, _)| names.contains(x)))
        .count();
    let has = |t: &str| n.block_types.iter().any(|x| x == t);
    let constraints = n.block_types.iter().any(|t| t.contains("Constraint"));
    let ragdoll = n.block_types.iter().any(|t| t.contains("Ragdoll"));
    let role = format!(
        "{}; {}; physics {}",
        match (has("NiControllerManager"), moving) {
            (false, _) => "no controller manager".to_string(),
            (true, 0) => format!("a manager whose {} sequences move no bone", seqs.len()),
            (true, k) => format!(
                "a manager, {} of its sequences moving bones",
                if k == seqs.len() { "all" } else { "some" }
            ),
        },
        if has("BSBehaviorGraphExtraData") {
            "a behaviour graph"
        } else {
            "no behaviour graph"
        },
        match (constraints, ragdoll) {
            (false, false) => "none",
            (true, false) => "constraints",
            (false, true) => "ragdoll",
            (true, true) => "constraints and ragdoll",
        }
    );
    let size = format!(
        "{} skinned shapes, {} bones",
        bucket(shapes),
        bucket(bones.len())
    );
    Ok((role, size))
}

fn skinned_models(
    store: &Store,
    vfs: &fo4_vfs::Vfs,
    m: &Manifest,
    b: &Budget,
    out: &mut String,
    names: &mut String,
) -> Result<(), E> {
    let _ = writeln!(
        out,
        "\n## 3. Skinned models of placed records other than actors (base records of the placed kinds with MODL)"
    );
    let mut by_base: BTreeMap<FormKey, Vec<u8>> = BTreeMap::new();
    let mut paths: BTreeMap<Vec<u8>, BTreeSet<String>> = BTreeMap::new();
    let winners: Vec<FormKey> = store
        .winners()
        .filter(|w| !w.deleted && fo4_scene::BASE_TYPES.contains(&w.signature))
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    for k in &winners {
        let work = b.child();
        let Some(r) = store.raw(k, &work)? else {
            continue;
        };
        let Some(x) = r.subrecords.iter().find(|x| x.signature == *b"MODL") else {
            continue;
        };
        let d = r.data(x);
        let raw = d.strip_suffix(&[0]).unwrap_or(d);
        let Ok(p) = fo4_scene::paths::resolve_with(
            fo4_scene::paths::Citation::RecordModel,
            raw,
            vfs,
            RuleLevel::Hypotheses,
            &work,
        ) else {
            continue;
        };
        by_base.insert(k.clone(), p.key.clone());
        paths
            .entry(p.key)
            .or_default()
            .insert(String::from_utf8_lossy(&r.signature).into_owned());
    }
    let mut skinned: BTreeMap<Vec<u8>, String> = BTreeMap::new();
    let mut models = Counts::new();
    for (p, kinds) in &paths {
        let work = b.child();
        let name = String::from_utf8_lossy(p).into_owned();
        let Ok(bytes) = vfs.read(&name, &work) else {
            continue;
        };
        let Ok(n) = fo4_nif::Nif::read(&bytes, &name, fo4_core::Game::Fallout4, &m.limits, &work)
        else {
            continue;
        };
        if !n.block_types.iter().any(|x| x == "BSSkin::Instance") {
            continue;
        }
        let (role, size) = skin_role(&n, &name, m, &work)?;
        let kinds = kinds.iter().cloned().collect::<Vec<_>>().join("+");
        bump(&mut models, role.clone());
        bump(&mut models, format!("  base kinds {kinds}"));
        bump(&mut models, format!("  {size}"));
        let _ = writeln!(
            names,
            "skinned model {name} kinds {kinds} | {role} | {size}"
        );
        skinned.insert(p.clone(), role);
    }
    let _ = writeln!(out, "models with skinned shapes\t{}", skinned.len());
    let _ = writeln!(
        out,
        "by what moves their bones (lines starting with two spaces: by base kind and by size):"
    );
    for (k, n) in &models {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    // Placements of those models over the load order.
    let mut placed = Counts::new();
    let mut total = 0u64;
    for (w, _) in store.placement_cells() {
        if w.signature != *b"REFR" || w.deleted {
            continue;
        }
        let work = b.child();
        let key = store.overrides().form_key(w.key, store.order());
        let Some((r, pl)) = store.raw_with_plugin(&key, &work)? else {
            continue;
        };
        let Some(x) = r.subrecords.iter().find(|x| x.signature == *b"NAME") else {
            continue;
        };
        let d = r.data(x);
        let Some(id) = d
            .get(..4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        else {
            continue;
        };
        let Some(base) = store.order().remap(pl, id, &r.source.file, x.offset)? else {
            continue;
        };
        let Some(role) = by_base.get(&base).and_then(|p| skinned.get(p)) else {
            continue;
        };
        total += 1;
        bump(&mut placed, role.clone());
    }
    let _ = writeln!(out, "placed references (REFR) of those models\t{total}");
    for (k, n) in &placed {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    Ok(())
}

fn shaft_parts(
    store: &Store,
    vfs: &fo4_vfs::Vfs,
    m: &Manifest,
    sk: &FormKey,
    b: &Budget,
    out: &mut String,
    names: &mut String,
) -> Result<(), E> {
    let _ = writeln!(out, "\n## 4. The shaft model's parts, block by block");
    let Some((r, pl)) = store.raw_with_plugin(sk, b)? else {
        let _ = writeln!(out, "reference not found");
        return Ok(());
    };
    let sub = |s: &[u8; 4]| r.subrecords.iter().filter(|x| x.signature == *s).count();
    let _ = writeln!(
        out,
        "reference subrecords: XMSP {}, XLRT {}, XESP {}, XLKR {}",
        sub(b"XMSP"),
        sub(b"XLRT"),
        sub(b"XESP"),
        sub(b"XLKR")
    );
    let base = r
        .subrecords
        .iter()
        .find(|x| x.signature == *b"NAME")
        .and_then(|x| {
            let d = r.data(x);
            let id = u32::from_le_bytes(d.get(..4)?.try_into().ok()?);
            store.order().remap(pl, id, &r.source.file, x.offset).ok()?
        });
    let Some(base) = base else {
        return Ok(());
    };
    let Some(br) = store.raw(&base, b)? else {
        return Ok(());
    };
    let Some(x) = br.subrecords.iter().find(|x| x.signature == *b"MODL") else {
        return Ok(());
    };
    let d = br.data(x);
    let p = fo4_scene::paths::resolve_with(
        fo4_scene::paths::Citation::RecordModel,
        d.strip_suffix(&[0]).unwrap_or(d),
        vfs,
        RuleLevel::Hypotheses,
        b,
    )?;
    let name = String::from_utf8_lossy(&p.key).into_owned();
    let bytes = vfs.read(&name, b)?;
    let n = fo4_nif::Nif::read(&bytes, &name, fo4_core::Game::Fallout4, &m.limits, b)?;
    let _ = writeln!(
        out,
        "base subrecords: MODS {}, PNAM {}, FNAM {}",
        br.subrecords
            .iter()
            .filter(|x| x.signature == *b"MODS")
            .count(),
        br.subrecords
            .iter()
            .filter(|x| x.signature == *b"PNAM")
            .count(),
        br.subrecords
            .iter()
            .filter(|x| x.signature == *b"FNAM")
            .count()
    );
    let mut c = Counts::new();
    // Controllers by their target, following each block's controller chain.
    let mut controlled: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for i in 0..n.blocks.len() {
        let mut next = links_of(&n, i, "Controller");
        let mut guard = 0;
        while let Some(cix) = next.pop() {
            guard += 1;
            if guard > 64 {
                break;
            }
            controlled
                .entry(i)
                .or_default()
                .push(ty(&n, cix).to_string());
            next.extend(links_of(&n, cix, "Next Controller"));
        }
    }
    for i in 0..n.blocks.len() {
        let t = ty(&n, i);
        match &n.blocks[i].decoded {
            Some(fo4_nif::Data::Mesh(_)) => {
                let skin = fo4_nif::skin::shape_skin(&n, i, &name, &m.limits, b)
                    .ok()
                    .flatten()
                    .is_some();
                let shader = links_of(&n, i, "Shader Property")
                    .first()
                    .map(|s| ty(&n, *s).to_string())
                    .unwrap_or_else(|| "no shader".into());
                let shader_ctl = links_of(&n, i, "Shader Property")
                    .first()
                    .and_then(|s| controlled.get(s))
                    .map(|v| v.join("+"))
                    .unwrap_or_else(|| "none".into());
                let alpha = !links_of(&n, i, "Alpha Property").is_empty();
                let why = if skin {
                    "drawn by the ride reading (skinned on the model's own nodes)"
                } else {
                    "left out by the ride reading (rigid shape)"
                };
                bump(
                    &mut c,
                    format!(
                        "shape {t}: {}, {shader}, alpha property {alpha}, shader controllers {shader_ctl}: {why}",
                        if skin { "skinned" } else { "rigid" }
                    ),
                );
                let _ = writeln!(
                    names,
                    "shaft shape {i} {:?} skinned {skin} {shader} {shader_ctl}",
                    node_name(&n, i)
                );
            }
            _ if t.contains("Light") && !t.contains("Controller") && !t.contains("Shader") => {
                bump(
                    &mut c,
                    format!(
                        "light block {t}, controllers {}: not drawn (the frame's lights are placed LIGH references; a light inside a model is not read)",
                        controlled.get(&i).map(|v| v.join("+")).unwrap_or_else(|| "none".into())
                    ),
                );
            }
            _ if t == "NiVisController" => {
                bump(&mut c, "visibility controller: its target's visibility over time is not read (UnsupportedAnimationRuntime, no rest rule)".to_string());
            }
            _ if t == "BSBehaviorGraphExtraData" => {
                bump(
                    &mut c,
                    "behaviour graph extra data: the graph that starts the sequences".to_string(),
                );
            }
            _ if t.starts_with("BSEffectShaderProperty") && t.ends_with("Controller") => {
                bump(
                    &mut c,
                    format!("{t}: an effect field run by the manager (effect-idle)"),
                );
            }
            _ => {}
        }
    }
    let seqs = fo4_scene::sequence::sequences(&n);
    let _ = writeln!(out, "controller sequences\t{}", seqs.len());
    for s in &seqs {
        let kinds: BTreeSet<&str> = s
            .blocks
            .iter()
            .filter_map(|(_, ip)| ip.map(|ip| ty(&n, ip)))
            .collect();
        bump(
            &mut c,
            format!(
                "sequence of cycle type {}: controlled blocks by interpolator kind [{}]",
                s.cycle,
                kinds.into_iter().collect::<Vec<_>>().join(", ")
            ),
        );
        let _ = writeln!(names, "shaft sequence {} {}", s.name, s.blocks.len());
    }
    for (k, v) in &c {
        let _ = writeln!(out, "  {v}\t{k}");
    }
    Ok(())
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str = "poses_census MANIFEST TIMELINE OUT.txt NAMES.txt [--seed N] [--frame S=OVERLAY]... [--models] [--shaft PLUGIN:HEX]";
    if args.len() < 4 {
        return Err(USAGE.into());
    }
    let (mut seed, mut shaft, mut models) = (1u64, None, false);
    let mut frames: Vec<Frame> = Vec::new();
    let mut it = args[4..].iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--seed" => seed = it.next().ok_or(USAGE)?.parse()?,
            "--frame" => {
                let (s, p) = it.next().ok_or(USAGE)?.split_once('=').ok_or(USAGE)?;
                frames.push(read_overlay(s, p)?);
            }
            "--shaft" => shaft = tl::parse_key(it.next().ok_or(USAGE)?),
            "--models" => models = true,
            _ => return Err(USAGE.into()),
        }
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let b = Budget::new(8 << 30).with_scope_limit(m.limits.max_memory);
    let _t = tl::Timeline::parse(&std::fs::read_to_string(&args[1])?)?;
    let mut out = String::new();
    let mut names = String::new();
    let _ = writeln!(
        out,
        "# Rest poses, markers, skinned models of placed records other than actors, the shaft's parts (poses_census; counts only)"
    );
    let readings = Readings {
        template_bits: true,
        leveled_seed: Some(seed),
        chance_none: true,
        use_all: true,
        head_slots: false,
    };
    let mut loader = Loader::new(&store, &vfs, &m.language, RuleLevel::Hypotheses, &b);

    // 1. Characters in the rest pose.
    let _ = writeln!(
        out,
        "\n## 1. Characters the frames draw in the rest pose (every look reading on, seed {seed})"
    );
    let mut rest: BTreeMap<FormKey, BTreeSet<String>> = BTreeMap::new();
    for f in &frames {
        let mut c = Counts::new();
        for (k, why) in &f.characters {
            if why.is_empty() {
                bump(&mut c, "posed");
            } else {
                bump(
                    &mut c,
                    why.split(": ").take(2).collect::<Vec<_>>().join(": "),
                );
                rest.entry(k.clone()).or_default().insert(f.second.clone());
            }
        }
        let _ = writeln!(
            out,
            "second {}: {} characters",
            f.second,
            f.characters.len()
        );
        for (k, n) in &c {
            let _ = writeln!(out, "  {n}\t{k}");
        }
    }
    let _ = writeln!(out, "distinct actors in the rest pose\t{}", rest.len());
    let mut classes = Counts::new();
    let mut missing_by_place = Counts::new();
    let mut races: BTreeMap<FormKey, usize> = BTreeMap::new();
    for (k, secs) in &rest {
        let src = fo4_core::SourceSpan::new("timeline", 0, 0);
        let a = actor::resolve_reference_with(&mut loader, k, &src, readings, &b)?;
        let Some(plan) = &a.plan else {
            bump(&mut classes, "no plan with the readings");
            continue;
        };
        let n = races.len();
        let ri = *races.entry(plan.race.clone()).or_insert(n);
        let keywords = match (&a.keyword_base, &a.base) {
            (Some((npc, by_bits)), _) => {
                char_graph::npc_keywords(&store, npc, &plan.race, *by_bits, &b)?
            }
            (None, Some(base)) => char_graph::actor_keywords(&store, base, &plan.race, &b)?,
            (None, None) => Err("no base".to_string()),
        };
        let source = match &a.keyword_base {
            Some((_, true)) => "keywords from the NPC_ the template bits chose",
            Some((_, false)) => "keywords from the one NPC_ the look settled",
            None => "keywords from the base",
        };
        let Ok(kw) = keywords else {
            bump(&mut classes, format!("{source}: refused"));
            continue;
        };
        let Some((sub, entries)) = char_graph::subgraphs(&store, &plan.race, &b)? else {
            bump(&mut classes, "no subgraph race");
            continue;
        };
        let word0: Vec<&char_graph::Entry> = entries
            .iter()
            .filter(|e| e.flags == 0 && e.target_keywords.is_empty())
            .collect();
        let empty0 = word0.iter().filter(|e| e.actor_keywords.is_empty()).count();
        let fit_any: BTreeMap<u32, usize> = entries
            .iter()
            .filter(|e| e.actor_keywords.iter().all(|x| kw.contains(x)))
            .fold(BTreeMap::new(), |mut m, e| {
                *m.entry(e.flags).or_default() += 1;
                m
            });
        let miss = |e: &char_graph::Entry| -> Vec<FormKey> {
            e.actor_keywords
                .iter()
                .filter(|x| !kw.contains(x))
                .cloned()
                .collect()
        };
        let least = word0.iter().map(|e| miss(e).len()).min().unwrap_or(0);
        let nearest: Vec<&&char_graph::Entry> =
            word0.iter().filter(|e| miss(e).len() == least).collect();
        let base = a
            .keyword_base
            .as_ref()
            .map(|x| x.0.clone())
            .or(a.base.clone());
        let places = match &base {
            Some(bk) => keyword_places(&store, bk, &b)?,
            None => BTreeMap::new(),
        };
        // Where the nearest entries' missing keywords are stored.
        let mut found: BTreeSet<String> = BTreeSet::new();
        for e in &nearest {
            let ps: BTreeSet<&str> = miss(e)
                .iter()
                .flat_map(|x| {
                    places
                        .get(x)
                        .map(|s| s.iter().copied().collect::<Vec<_>>())
                        .unwrap_or_else(|| vec!["nowhere in its records"])
                })
                .collect();
            found.insert(ps.into_iter().collect::<Vec<_>>().join(" + "));
        }
        for f in &found {
            bump(&mut missing_by_place, f.clone());
        }
        // Entries whose actor keywords all are stored somewhere in the
        // actor's records (chain, outfit, inventory).
        let stored: BTreeSet<&FormKey> = places.keys().chain(kw.iter()).collect();
        let fit_stored = word0
            .iter()
            .filter(|e| e.actor_keywords.iter().all(|x| stored.contains(x)))
            .count();
        if ri + 1 == races.len() {
            for e in &entries {
                let _ = writeln!(
                    names,
                    "entry race {} index {} flags {:#x} actor [{}] target [{}] behaviour {} folders {:?}",
                    tl::key_text(&sub),
                    e.index,
                    e.flags,
                    e.actor_keywords.iter().map(|x| edid_of(&store, x, &b)).collect::<Vec<_>>().join(" "),
                    e.target_keywords.iter().map(|x| edid_of(&store, x, &b)).collect::<Vec<_>>().join(" "),
                    e.behavior,
                    e.folders
                );
            }
        }
        let near = if word0.is_empty() {
            "no entry of flag word 0 without target keywords to be near".to_string()
        } else {
            format!(
                "nearest entry of flag word 0 misses {least} keyword(s) ({} such entries); entries of flag word 0 fitting the keywords stored anywhere in its records: {fit_stored}",
                nearest.len()
            )
        };
        let one = match char_graph::pick_entry_with(&entries, &kw, true) {
            Ok((_, 1)) => "picks an entry of flag word 1".to_string(),
            Ok((_, w)) => format!("picks an entry of flag word {w}"),
            Err(why) => format!("refuses: {why}"),
        };
        let class = format!(
            "race {ri}, {source}, {} keywords; subgraph race with {} entries, {} of flag word 0 without target keywords ({} of them without actor keywords); entries the keywords fit by flag word {:?}; {near}; --actor-subgraph-flag-one {one}",
            kw.len(),
            entries.len(),
            word0.len(),
            empty0,
            fit_any,
        );
        bump(&mut classes, class.clone());
        let _ = writeln!(
            names,
            "rest {} {} seconds {:?} race {} ({}) subgraph race {} ({}) | {class} | keywords [{}] | nearest missing [{}] | stored where {:?}",
            tl::key_text(k),
            edid_of(&store, k, &b),
            secs,
            tl::key_text(&plan.race),
            edid_of(&store, &plan.race, &b),
            tl::key_text(&sub),
            edid_of(&store, &sub, &b),
            kw.iter()
                .map(|x| edid_of(&store, x, &b))
                .collect::<Vec<_>>()
                .join(" "),
            nearest
                .iter()
                .map(|e| format!(
                    "entry {}: {}",
                    e.index,
                    miss(e)
                        .iter()
                        .map(|x| edid_of(&store, x, &b))
                        .collect::<Vec<_>>()
                        .join(" ")
                ))
                .collect::<Vec<_>>()
                .join("; "),
            found
        );
    }
    let _ = writeln!(out, "by race, keyword source and subgraph entries:");
    for (c, n) in &classes {
        let _ = writeln!(out, "  {n}\t{c}");
    }
    let _ = writeln!(
        out,
        "where the nearest entries' missing keywords are stored (per actor):"
    );
    for (c, n) in &missing_by_place {
        let _ = writeln!(out, "  {n}\t{c}");
    }

    // 2. Markers.
    let _ = writeln!(
        out,
        "\n## 2. Markers of the frames (every look reading on, seed {seed})"
    );
    let mut marked: BTreeMap<FormKey, BTreeSet<String>> = BTreeMap::new();
    for f in &frames {
        let mut c = Counts::new();
        for (k, why) in &f.markers {
            bump(&mut c, marker_class(why));
            marked
                .entry(k.clone())
                .or_default()
                .insert(f.second.clone());
        }
        let _ = writeln!(out, "second {}: {} markers", f.second, f.markers.len());
        for (k, n) in &c {
            let _ = writeln!(out, "  {n}\t{k}");
        }
    }
    let _ = writeln!(out, "distinct actors drawn as markers\t{}", marked.len());
    let mut families = Counts::new();
    let mut by_part = Counts::new();
    let mut per_actor = Counts::new();
    let mut graphs = Counts::new();
    let mut graph_seen: BTreeSet<(FormKey, String)> = BTreeSet::new();
    for (k, secs) in &marked {
        let src = fo4_core::SourceSpan::new("timeline", 0, 0);
        let a = actor::resolve_reference_with(&mut loader, k, &src, readings, &b)?;
        let Some(plan) = &a.plan else {
            bump(&mut per_actor, "no plan");
            continue;
        };
        let role = |file: &str| -> &'static str {
            let f = norm_path(file.as_bytes());
            let same = |p: &[u8]| {
                let p = norm_path(p);
                p == f || f.ends_with(&format!("/{p}")) || p.ends_with(&format!("/{f}"))
            };
            if same(&plan.skeleton) || same(&loader.assets.nodes[plan.skeleton_asset].path) {
                return "the skeleton";
            }
            for p in &plan.parts {
                if same(&loader.assets.nodes[p.asset].path) || same(&p.path) {
                    return if p.skin {
                        "the skin (body)"
                    } else {
                        "an outfit piece"
                    };
                }
            }
            if let Some(h) = &plan.head {
                for p in &h.parts {
                    if same(&loader.assets.nodes[p.asset].path) || same(&p.path) {
                        return "a head part";
                    }
                }
            }
            "a texture or another file"
        };
        // The behaviour graph a part model's extra data names, against the
        // race's root graph.
        let race_graph = store
            .raw(&plan.race, &b)?
            .and_then(|r| char_graph::race_project(&r, plan.section))
            .and_then(|t| char_graph::project_chain(&vfs, &t, &b).ok())
            .map(|(_, g)| norm_path(g.as_bytes()));
        for e in a
            .blockers
            .iter()
            .filter(|e| e.found.ends_with(";rest=behavior-graph"))
        {
            if !graph_seen.insert((k.clone(), e.source.file.clone())) {
                continue;
            }
            let work = b.child();
            let key = norm_path(e.source.file.as_bytes());
            let Ok(bytes) = vfs.read(&key, &work) else {
                bump(&mut graphs, "model not read");
                continue;
            };
            let Ok(n) =
                fo4_nif::Nif::read(&bytes, &key, fo4_core::Game::Fallout4, &m.limits, &work)
            else {
                bump(&mut graphs, "model not decoded");
                continue;
            };
            for blk in &n.blocks {
                if n.block_types
                    .get(blk.type_index as usize)
                    .map(String::as_str)
                    != Some("BSBehaviorGraphExtraData")
                {
                    continue;
                }
                let Some(d) = &blk.decoded else { continue };
                let fs = fo4_scene::model_policy::fields(d);
                let text = |name: &str| {
                    fs.iter()
                        .find(|f| f.name == name)
                        .and_then(|f| match f.value {
                            fo4_nif::Value::StringIndex(i) => n.strings.get(i as usize).cloned(),
                            _ => None,
                        })
                };
                let graph = text("Behavior Graph File").map(|g| norm_path(&g));
                let controls = fs
                    .iter()
                    .find(|f| f.name == "Controls Base Skeleton")
                    .map(|f| format!("{:?}", f.value));
                let same = match (&graph, &race_graph) {
                    (Some(g), Some(r)) => {
                        if r.ends_with(g.as_str()) || g.ends_with(r.as_str()) {
                            "names the race's root graph file"
                        } else {
                            "names another graph file"
                        }
                    }
                    (None, _) => "names no graph file",
                    (_, None) => "the race's root graph not found",
                };
                // The model graph's root start state.
                let model = key.strip_prefix("meshes/").unwrap_or(&key);
                let start = match fo4_scene::anim_graph::model_graph(&vfs, model, &m.limits, &work)
                {
                    Err(_) => "its graph not read".to_string(),
                    Ok(g) => {
                        let st = g
                            .machine
                            .state(g.machine.start)
                            .and_then(|s| s.sequence.clone());
                        let effect_seqs: BTreeSet<String> = effect_sequences(&n);
                        match st {
                            None => "its graph's start state plays no sequence".into(),
                            Some(q) if effect_seqs.contains(&q.to_ascii_lowercase()) => {
                                let _ =
                                    writeln!(names, "graph start {} plays {q}", tl::key_text(k));
                                "its graph's start state plays a sequence driving an effect controller".into()
                            }
                            Some(q) => {
                                let _ =
                                    writeln!(names, "graph start {} plays {q}", tl::key_text(k));
                                "its graph's start state plays another sequence".into()
                            }
                        }
                    }
                };
                bump(
                    &mut graphs,
                    format!(
                        "{} model ({}): {same}; {start}; controls base skeleton {}",
                        role(&e.source.file),
                        if n.block_types.iter().any(|t| t == "NiControllerManager") {
                            "with a controller manager"
                        } else {
                            "no controller manager"
                        },
                        controls.unwrap_or_default()
                    ),
                );
                let _ = writeln!(
                    names,
                    "graph {} model {} names {:?} race graph {:?}",
                    tl::key_text(k),
                    e.source.file,
                    graph,
                    race_graph
                );
            }
        }
        let open: Vec<&fo4_core::Error> = a
            .blockers
            .iter()
            .filter(|e| fo4_scene::hypotheses::covering(e).is_none())
            .collect();
        let mut fams: BTreeSet<String> = BTreeSet::new();
        for e in &open {
            let fam = family(e);
            let part = role(&e.source.file);
            bump(&mut families, format!("{fam}\t{}", e.code));
            fams.insert(format!("{fam} on {part}"));
            let _ = writeln!(
                names,
                "marker {} {} seconds {secs:?} blocker {} [{}] in {} ({part}): {}",
                tl::key_text(k),
                edid_of(&store, k, &b),
                e.code,
                fam,
                e.source.file,
                e.found
            );
        }
        let parts: BTreeSet<String> = fams.iter().cloned().collect();
        bump(
            &mut per_actor,
            if parts.is_empty() {
                format!("ready with the readings ({:?})", a.state)
                    .split(" {")
                    .next()
                    .unwrap_or("")
                    .to_string()
            } else {
                parts.into_iter().collect::<Vec<_>>().join(" + ")
            },
        );
        for f in fams {
            bump(&mut by_part, f);
        }
    }
    let _ = writeln!(
        out,
        "marker actors by the open blocker families of their models (family on part):"
    );
    for (k, n) in &per_actor {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let _ = writeln!(out, "actors per family and part:");
    for (k, n) in &by_part {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let _ = writeln!(
        out,
        "behaviour graph extra data of those actors' models (per actor and model):"
    );
    for (k, n) in &graphs {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let _ = writeln!(out, "open blockers by family and code (over those actors):");
    for (k, n) in &families {
        let _ = writeln!(out, "  {n}\t{k}");
    }

    // 1b. Subgraph entries over the load order, by flag word.
    let _ = writeln!(
        out,
        "\n### Races of the load order holding subgraph entries (SAKD/STKD/SGNM/SAPT/SRAF runs)"
    );
    let mut holders: BTreeMap<FormKey, Vec<char_graph::Entry>> = BTreeMap::new();
    let mut users = Counts::new();
    let all_races: Vec<FormKey> = store
        .winners()
        .filter(|w| w.signature == *b"RACE" && !w.deleted)
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    for r in &all_races {
        let work = b.child();
        let Some((sub, entries)) = char_graph::subgraphs(&store, r, &work)? else {
            continue;
        };
        if entries.is_empty() {
            bump(&mut users, "races whose subgraph race holds no entry");
            continue;
        }
        bump(
            &mut users,
            if &sub == r {
                "races holding their own entries"
            } else {
                "races naming another race's entries (SRAC)"
            },
        );
        holders.entry(sub).or_insert(entries);
    }
    let _ = writeln!(out, "RACE records\t{}", all_races.len());
    for (k, n) in &users {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let mut kinds = Counts::new();
    let mut shapes = Counts::new();
    for entries in holders.values() {
        let mut words: BTreeMap<(u32, bool), (u64, u64)> = BTreeMap::new();
        for e in entries {
            let w = words
                .entry((e.flags, !e.target_keywords.is_empty()))
                .or_default();
            w.0 += 1;
            if e.actor_keywords.is_empty() {
                w.1 += 1;
            }
            bump(
                &mut kinds,
                format!(
                    "flag word {:#x}, target keywords {}, actor keywords {}",
                    e.flags,
                    if e.target_keywords.is_empty() {
                        "none"
                    } else {
                        "some"
                    },
                    if e.actor_keywords.is_empty() {
                        "none"
                    } else {
                        "some"
                    }
                ),
            );
        }
        let has = |w: u32| words.contains_key(&(w, false));
        bump(
            &mut shapes,
            format!(
                "word 0 without target keywords: {}; word 1 without target keywords: {}; word 1 with target keywords: {}",
                if has(0) { "yes" } else { "no" },
                if has(1) { "yes" } else { "no" },
                if words.contains_key(&(1, true)) { "yes" } else { "no" }
            ),
        );
    }
    let _ = writeln!(
        out,
        "races holding entries\t{}\nentries by flag word and keywords (all holders):",
        holders.len()
    );
    for (k, n) in &kinds {
        let _ = writeln!(out, "  {n}\t{k}");
    }
    let _ = writeln!(out, "holders by the entries they have:");
    for (k, n) in &shapes {
        let _ = writeln!(out, "  {n}\t{k}");
    }

    // 3. Skinned models of placed records other than actors.
    if models {
        skinned_models(&store, &vfs, &m, &b, &mut out, &mut names)?;
    }
    // 4. The shaft's model, block by block.
    if let Some(sk) = &shaft {
        shaft_parts(&store, &vfs, &m, sk, &b, &mut out, &mut names)?;
    }

    std::fs::write(&args[2], out)?;
    std::fs::write(&args[3], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
