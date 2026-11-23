#![forbid(unsafe_code)]
//! Census of what the data offers a run timeline's dead actors, sitters,
//! moving platforms and image space (`spec/seen-census.txt`, the seen step:
//! `spec/timeline.md`, Deaths and furniture, The elevator second;
//! `spec/light.md`, Image space). Counts, stored numbers and descriptions
//! only in `OUT.txt`; record keys, editor ids, file, node, sequence and
//! clip names only in `NAMES.txt` (never versioned).
//!
//! `seen_census MANIFEST TIMELINE OUT.txt NAMES.txt [--elevator PLUGIN:HEX ...]`
//!
//! 1. deaths: the timeline's `death` lines, the dead actors' bases and
//!    races, and in each race's behaviour graph files (the root graph and
//!    every behaviour file it references) the clip generators and nodes
//!    whose clip or node name holds the word for death, the event names
//!    holding it, and the objects of ragdoll classes; the race's skeleton
//!    files' ragdoll objects;
//! 2. furniture: every FURN's SNAM size against its MNAM bits, the
//!    furniture models' marker blocks, and for each `sit` line of the
//!    timeline the furniture's markers, the seat
//!    `timeline_sit_marker_nearest_v1` gives against the track's point,
//!    and the clip `char_graph_furniture_entry_keywords_v1` reaches for
//!    the player's race;
//! 3. moving platforms (`--elevator`): the reference's model, its nodes
//!    with collision, its sequences and the travel of each node they move,
//!    the collision readiness of the reference, the collision-marker boxes
//!    inside its object bounds;
//! 4. image space: the image space records the timeline's interior cells
//!    name (XCIM) and the climate weathers' slots name (IMSP), word by
//!    word, and the ranges over every IMGS of the load order.
use fo4_core::{Budget, Game, Manifest};
use fo4_plugin::{typed::Store, FormKey, Record};
use fo4_scene::timeline::{self as tl, BodyNote, Sitter, Timeline, What};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;

fn sub<'r>(r: &'r Record, sig: &[u8; 4]) -> Option<&'r [u8]> {
    r.subrecords
        .iter()
        .find(|s| s.signature == *sig)
        .map(|s| r.data(s))
}
fn words(d: &[u8]) -> Vec<f32> {
    d.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}
fn form(store: &Store, r: &Record, plugin: usize, sig: &[u8; 4], at: usize) -> Option<FormKey> {
    let s = r.subrecords.iter().find(|s| s.signature == *sig)?;
    let d = r.data(s);
    let w = d.get(at..at + 4)?;
    store
        .order()
        .remap(
            plugin,
            u32::from_le_bytes([w[0], w[1], w[2], w[3]]),
            &r.source.file,
            s.offset,
        )
        .ok()
        .flatten()
}
fn edid(r: &Record) -> String {
    sub(r, b"EDID")
        .map(|d| String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned())
        .unwrap_or_default()
}
fn state_label(s: &fo4_scene::State) -> String {
    format!("{s:?}")
        .split([' ', '{'])
        .next()
        .unwrap_or_default()
        .to_string()
}
fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}
fn nums(v: &[f32]) -> String {
    v.iter()
        .map(|x| format!("{x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The graph files of a race's project: the root graph and every behaviour
/// file reached through behaviour references (at most 64).
fn graph_files(vfs: &fo4_vfs::Vfs, project: &str, b: &Budget) -> Vec<String> {
    let Ok((pk, root)) = fo4_scene::char_graph::project_chain(vfs, project, b) else {
        return Vec::new();
    };
    let folder = pk.rsplit_once('/').map_or("", |x| x.0).to_string();
    let mut out = vec![root.clone()];
    let mut i = 0;
    while i < out.len() && out.len() < 64 {
        let f = out[i].clone();
        i += 1;
        let Ok(bytes) = vfs.read(&f, &b.child()) else {
            continue;
        };
        let Ok(c) = fo4_havok::Container::read(&bytes, &f, 0, &b.child()) else {
            continue;
        };
        for k in 0..c.objects.len() {
            if c.objects[k].class != "hkbBehaviorReferenceGenerator" {
                continue;
            }
            if let Ok(fo4_havok::Node::Reference { behavior }) =
                fo4_havok::Node::read(&c, k, &f, 0, &b.child())
            {
                let key = format!("{folder}/{}", behavior.replace('\\', "/")).to_ascii_lowercase();
                if !out.contains(&key) {
                    out.push(key);
                }
            }
        }
    }
    out
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str =
        "seen_census MANIFEST TIMELINE OUT.txt NAMES.txt [--elevator PLUGIN:HEX ...]";
    if args.len() < 4 {
        return Err(USAGE.into());
    }
    let mut elevators = Vec::new();
    let mut rest = args[4..].iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--elevator" => elevators
                .push(tl::parse_key(rest.next().ok_or(USAGE)?).ok_or("a key as plugin:XXXXXXXX")?),
            _ => return Err(USAGE.into()),
        }
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = fo4_vfs::Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let t = Timeline::parse(&std::fs::read_to_string(&args[1])?)?;
    let mut out = String::new();
    let mut names = String::new();
    let _ = writeln!(out, "# What the data offers a run timeline's dead actors, sitters, moving platforms and image space (seen_census; counts and stored numbers only)");

    // 1. Deaths.
    let _ = writeln!(out, "\n## 1. Deaths");
    let mut dead: Vec<(f64, FormKey)> = Vec::new();
    let mut sits: Vec<(f64, Sitter, FormKey)> = Vec::new();
    for e in &t.events {
        if let What::Note(text) = &e.what {
            match tl::body_note(text) {
                Some(BodyNote::Death(Some(k))) => dead.push((e.t, k)),
                Some(BodyNote::Sit(w, f)) => sits.push((e.t, w, f)),
                _ => {}
            }
        }
    }
    let _ = writeln!(out, "death lines: {}", dead.len());
    let mut races: BTreeMap<FormKey, u32> = BTreeMap::new();
    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
    for (s, k) in &dead {
        let refr = store.raw_with_plugin(k, &b)?;
        let base = refr
            .as_ref()
            .and_then(|(r, p)| form(&store, r, *p, b"NAME", 0));
        let bs = base.as_ref().and_then(|x| store.signature(x)).map(sig);
        *kinds
            .entry(format!(
                "placed {} with a base {}",
                refr.as_ref()
                    .map_or("none".into(), |(r, _)| sig(r.signature)),
                bs.clone().unwrap_or("none".into())
            ))
            .or_default() += 1;
        let race = match (&base, bs.as_deref()) {
            (Some(bk), Some("NPC_")) => store
                .raw_with_plugin(bk, &b)?
                .and_then(|(r, p)| form(&store, &r, p, b"RNAM", 0)),
            _ => None,
        };
        let _ = writeln!(
            names,
            "death {s} {} base {} race {}",
            tl::key_text(k),
            base.as_ref().map_or("-".into(), tl::key_text),
            race.as_ref().map_or("-".into(), |r| format!(
                "{} {}",
                tl::key_text(r),
                store
                    .raw(r, &b)
                    .ok()
                    .flatten()
                    .map(|x| edid(&x))
                    .unwrap_or_default()
            ))
        );
        if let Some(r) = race {
            *races.entry(r).or_default() += 1;
        }
    }
    for (k, n) in &kinds {
        let _ = writeln!(out, "dead actors: {k}: {n}");
    }
    let _ = writeln!(
        out,
        "distinct races of the dead (an NPC_ base's RNAM): {}",
        races.len()
    );
    for (ri, (race, n)) in races.iter().enumerate() {
        let Some(r) = store.raw(race, &b)? else {
            continue;
        };
        let _ = writeln!(names, "race {ri}: {} {}", tl::key_text(race), edid(&r));
        for section in 0..2 {
            let Some(project) = fo4_scene::char_graph::race_project(&r, section) else {
                let _ = writeln!(
                    out,
                    "race {ri} ({n} dead), section {section}: no behaviour project"
                );
                continue;
            };
            let files = graph_files(&vfs, &project, &b);
            let (mut clips, mut death_clips, mut death_nodes, mut death_events, mut ragdoll) =
                (0u32, 0u32, 0u32, 0u32, BTreeMap::<String, u32>::new());
            let mut death_states = 0u32;
            for f in &files {
                let Ok(bytes) = vfs.read(f, &b.child()) else {
                    continue;
                };
                let Ok(c) = fo4_havok::Container::read(&bytes, f, 0, &b.child()) else {
                    continue;
                };
                if let Some(si) = fo4_havok::only_object(&c, fo4_havok::GRAPH_STRINGS) {
                    if let Ok(ev) = fo4_havok::graph_events(&c, si, f, 0, &b.child()) {
                        for e in ev {
                            if String::from_utf8_lossy(e)
                                .to_ascii_lowercase()
                                .contains("death")
                            {
                                death_events += 1;
                                let _ = writeln!(
                                    names,
                                    "race {ri} s{section} event {}",
                                    String::from_utf8_lossy(e)
                                );
                            }
                        }
                    }
                }
                for k in 0..c.objects.len() {
                    let class = c.objects[k].class;
                    if class.to_ascii_lowercase().contains("ragdoll") {
                        *ragdoll.entry(class.to_string()).or_default() += 1;
                    }
                    let name = fo4_havok::node_name(&c, k, f, 0)
                        .map(|x| String::from_utf8_lossy(x).to_ascii_lowercase())
                        .unwrap_or_default();
                    if class == fo4_havok::STATE_INFO && name.contains("death") {
                        death_states += 1;
                    }
                    if class == "hkbClipGenerator" {
                        clips += 1;
                        if let Ok(fo4_havok::Node::Clip(g)) =
                            fo4_havok::Node::read(&c, k, f, 0, &b.child())
                        {
                            if g.clip.to_ascii_lowercase().contains("death")
                                || name.contains("death")
                            {
                                death_clips += 1;
                                let _ =
                                    writeln!(names, "race {ri} s{section} clip {} in {f}", g.clip);
                            }
                        }
                    } else if name.contains("death") {
                        death_nodes += 1;
                    }
                }
            }
            let _ = writeln!(
                out,
                "race {ri} ({n} dead), section {section}: {} graph files (root and referenced behaviour files); {clips} clip generators, {death_clips} whose clip or name holds the word for death; {death_states} state infos and {death_nodes} other nodes named with it; {death_events} event names holding it; ragdoll classes: {}",
                files.len(),
                if ragdoll.is_empty() {
                    "none".into()
                } else {
                    ragdoll
                        .iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
        // The race's skeleton files (ANAM texts) and their ragdoll objects.
        for (ai, s) in r
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"ANAM")
            .enumerate()
        {
            let d = r.data(s);
            let text = String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned();
            let Some(hkx) = fo4_scene::actor::clip::skeleton_hkx_path(text.as_bytes()) else {
                continue;
            };
            let key = format!(
                "meshes/{}",
                String::from_utf8_lossy(&hkx).replace('\\', "/")
            )
            .to_ascii_lowercase();
            let mut rag = BTreeMap::<String, u32>::new();
            let read = vfs.read(&key, &b.child());
            if let Ok(bytes) = &read {
                if let Ok(c) = fo4_havok::Container::read(bytes, &key, 0, &b.child()) {
                    for o in &c.objects {
                        if o.class.to_ascii_lowercase().contains("ragdoll") {
                            *rag.entry(o.class.to_string()).or_default() += 1;
                        }
                    }
                }
            }
            let _ = writeln!(names, "race {ri} skeleton {ai}: {key}");
            let _ = writeln!(
                out,
                "race {ri} skeleton {ai}: Havok file beside it {}; ragdoll classes: {}",
                if read.is_ok() { "read" } else { "not mounted" },
                if rag.is_empty() {
                    "none".into()
                } else {
                    rag.iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
    }

    // 2. Furniture.
    let _ = writeln!(out, "\n## 2. Furniture");
    let furns: Vec<FormKey> = store
        .winners()
        .filter(|w| w.signature == *b"FURN")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut shape: BTreeMap<String, u32> = BTreeMap::new();
    let mut agree = (0u32, 0u32);
    let mut models: BTreeSet<String> = BTreeSet::new();
    for k in &furns {
        let Some((r, p)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        let snam = sub(&r, b"SNAM").map(<[u8]>::len);
        let bits = sub(&r, b"MNAM")
            .filter(|d| d.len() == 4)
            .map(|d| (u32::from_le_bytes([d[0], d[1], d[2], d[3]]) & 0xff_ffff).count_ones());
        let read = fo4_scene::furniture::markers(&store, &r, p)?;
        match &read {
            Ok(ms) => {
                *shape
                    .entry(format!(
                        "SNAM {} bytes: {} markers read",
                        snam.unwrap_or(0),
                        ms.len()
                    ))
                    .or_default() += 1;
                if bits == Some(ms.len() as u32) {
                    agree.0 += 1;
                } else {
                    agree.1 += 1;
                }
            }
            Err(why) => *shape.entry(format!("not read: {why}")).or_default() += 1,
        }
        for sg in [b"MODL", b"XMRK"] {
            if let Some(d) = sub(&r, sg) {
                let m =
                    String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).to_ascii_lowercase();
                if !m.is_empty() {
                    models.insert(format!("meshes/{}", m.replace('\\', "/")));
                }
            }
        }
    }
    let _ = writeln!(out, "FURN records: {}", furns.len());
    for (k, v) in &shape {
        let _ = writeln!(out, "  {k}: {v}");
    }
    let _ = writeln!(
        out,
        "  markers read equal to the MNAM low 24 bits set: {} records; different: {}",
        agree.0, agree.1
    );
    let (mut with_blocks, mut entries, mut unread) = (0u32, 0u32, 0u32);
    for mk in &models {
        let Ok(bytes) = vfs.read(mk, &b.child()) else {
            unread += 1;
            continue;
        };
        let Ok(n) = fo4_nif::Nif::read(&bytes, mk, Game::Fallout4, &m.limits, &b.child()) else {
            unread += 1;
            continue;
        };
        let mut found = false;
        for blk in &n.blocks {
            let ty = n
                .block_types
                .get(blk.type_index as usize)
                .map_or("", String::as_str);
            if ty == "BSFurnitureMarkerNode" || ty == "BSFurnitureMarker" {
                found = true;
                if let Some(fo4_nif::Data::Fields(f)) = &blk.decoded {
                    entries += f.iter().filter(|x| x.name == "Heading").count() as u32;
                }
            }
        }
        with_blocks += u32::from(found);
    }
    let _ = writeln!(
        out,
        "furniture and marker models (MODL, XMRK): {}; holding a furniture marker block: {with_blocks} ({entries} entries); not read: {unread}",
        models.len()
    );
    // The player's race, for the sitters' clip.
    let player_base = FormKey {
        plugin: "fallout4.esm".into(),
        object_id: 7,
    };
    let race = match store.raw_with_plugin(&player_base, &b)? {
        Some((r, p)) => form(&store, &r, p, b"RNAM", 0),
        None => None,
    };
    let _ = writeln!(out, "sit lines: {}", sits.len());
    let on = fo4_scene::char_graph::Switches {
        start: true,
        subgraph: true,
        folder: true,
        cycle: false,
        sit: true,
        flag_one: false,
    };
    let frames: Vec<f64> = sits.iter().map(|s| s.0).collect();
    for (i, (s, who, f)) in sits.iter().enumerate() {
        let near = t.at(frames[i]).player.map(|p| p.pos);
        let seat = match near {
            Some(p) => Some((p, fo4_scene::furniture::seat_near(&store, f, p, &b)?)),
            None => None,
        };
        let base = store
            .raw_with_plugin(f, &b)?
            .and_then(|(r, p)| form(&store, &r, p, b"NAME", 0));
        let markers = match &base {
            Some(bk) => match store.raw_with_plugin(bk, &b)? {
                Some((r, p)) => Some(fo4_scene::furniture::markers(&store, &r, p)?),
                None => None,
            },
            None => None,
        };
        let marker_text = match &markers {
            Some(Ok(ms)) => ms
                .iter()
                .map(|x| {
                    format!(
                        "offset {} heading {}{}",
                        nums(&x.offset),
                        x.heading,
                        if x.keyword.is_some() {
                            " with a keyword"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
            Some(Err(w)) => format!("not read: {w}"),
            None => "no base".into(),
        };
        let seat_text = match &seat {
            Some((p, Ok(x))) => format!(
                "marker {} of {} at {:.1} units from the track's point",
                x.index,
                x.of,
                (0..3)
                    .map(|j| (x.pos[j] - p[j]).powi(2))
                    .sum::<f64>()
                    .sqrt()
            ),
            Some((_, Err(w))) => format!("no seat: {w}"),
            None => "no track point".into(),
        };
        let clip = match (&race, &base) {
            (Some(rk), Some(bk)) => {
                let kw = BTreeSet::new();
                let input = fo4_scene::char_graph::Actor {
                    race: rk,
                    section: 0,
                    keywords: Ok(&kw),
                };
                match fo4_scene::char_graph::furniture_clip(&vfs, &store, &input, bk, &on, &b)? {
                    Ok(c) => {
                        let _ = writeln!(
                            names,
                            "sit {i}: clip {} entry {:?}",
                            c.clip,
                            c.subgraph.as_ref().map(|e| (&e.behavior, e.index))
                        );
                        format!(
                            "reaches a clip through subgraph entry {} in {} generators (mode {}, {} ties)",
                            c.subgraph.as_ref().map_or(0, |e| e.index),
                            c.steps.len(),
                            c.generator.mode,
                            c.ties
                        )
                    }
                    Err(r) => format!("refused, {}: {}", r.code, r.detail),
                }
            }
            _ => "no race or base".into(),
        };
        let _ = writeln!(
            names,
            "sit {i} at {s}: {} in {} base {}",
            match who {
                Sitter::Player => "player".into(),
                Sitter::Actor(k) => tl::key_text(k),
            },
            tl::key_text(f),
            base.as_ref().map_or("-".into(), tl::key_text)
        );
        let _ = writeln!(
            out,
            "sit {i} at {s} s, {}: markers {marker_text}; seat {seat_text}; the player's race (section 0, no actor keywords): {clip}",
            match who {
                Sitter::Player => "the player",
                Sitter::Actor(_) => "an actor",
            }
        );
    }

    // 3. Moving platforms.
    let _ = writeln!(
        out,
        "\n## 3. Moving platforms (the references the caller names)"
    );
    for (ei, ek) in elevators.iter().enumerate() {
        let Some((r, p)) = store.raw_with_plugin(ek, &b)? else {
            let _ = writeln!(out, "platform {ei}: no winning record");
            continue;
        };
        let base = form(&store, &r, p, b"NAME", 0);
        let Some(bk) = base else { continue };
        let Some((br, _)) = store.raw_with_plugin(&bk, &b)? else {
            continue;
        };
        let model = sub(&br, b"MODL")
            .map(|d| String::from_utf8_lossy(d.strip_suffix(&[0]).unwrap_or(d)).into_owned())
            .unwrap_or_default();
        let _ = writeln!(
            names,
            "platform {ei}: {} {} base {} {} model {model}",
            tl::key_text(ek),
            edid(&r),
            tl::key_text(&bk),
            edid(&br)
        );
        let place = sub(&r, b"DATA").map(words).unwrap_or_default();
        let obnd: Vec<i16> = sub(&br, b"OBND")
            .map(|d| {
                d.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| i16::from_le_bytes(*c))
                    .collect()
            })
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "platform {ei}: a placed {} of a base {}; placement {}; object bounds {:?}",
            sig(r.signature),
            sig(br.signature),
            nums(&place),
            obnd
        );
        let mk = format!("meshes/{}", model.replace('\\', "/")).to_ascii_lowercase();
        let Ok(bytes) = vfs.read(&mk, &b.child()) else {
            let _ = writeln!(out, "  model not mounted");
            continue;
        };
        let n = fo4_nif::Nif::read(&bytes, &mk, Game::Fallout4, &m.limits, &b.child())?;
        let mut tally: BTreeMap<String, u32> = BTreeMap::new();
        for blk in &n.blocks {
            let ty = n
                .block_types
                .get(blk.type_index as usize)
                .cloned()
                .unwrap_or_default();
            if ty.starts_with("bhk")
                || ty == "NiNode"
                || ty.starts_with("BSSkin")
                || ty == "BSBehaviorGraphExtraData"
                || ty == "NiControllerSequence"
                || ty == "NiTextKeyExtraData"
            {
                *tally.entry(ty).or_default() += 1;
            }
        }
        let _ = writeln!(
            out,
            "  model blocks: {}",
            tally
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let seqs = fo4_scene::sequence::sequences(&n);
        let mut moved: BTreeSet<String> = BTreeSet::new();
        for (si, s) in seqs.iter().enumerate() {
            let _ = writeln!(names, "platform {ei} sequence {si}: {}", s.name);
            let mut travel = Vec::new();
            for (node, _) in &s.blocks {
                if let Some(keys) = fo4_scene::sequence::node_keys(&n, s, node, &b)? {
                    let first = keys.translation.sample(f64::from(s.start));
                    let last = keys.translation.sample(f64::from(s.stop));
                    if let (Some(a), Some(z)) = (first, last) {
                        let d = [z[0] - a[0], z[1] - a[1], z[2] - a[2]];
                        if d.iter().any(|x| x.abs() > 1.) {
                            let ni = moved.iter().position(|x| x == node).unwrap_or(moved.len());
                            moved.insert(node.clone());
                            let _ = writeln!(
                                names,
                                "platform {ei} sequence {si} node {node}: from {} to {}",
                                nums(&a),
                                nums(&z)
                            );
                            let stored = fo4_scene::sequence::node(&n, node)
                                .map_or("not found".into(), |x| nums(&x.1));
                            travel.push(format!(
                                "a node moved by {} ({} translation keys, kind {}; from {} to {}, its stored translation {stored})",
                                nums(&d),
                                keys.translation.keys.len(),
                                keys.translation.kind,
                                nums(&a),
                                nums(&z)
                            ));
                            let _ = ni;
                        }
                    }
                }
            }
            let _ = writeln!(
                out,
                "  sequence {si}: {} to {} s, cycle {}, frequency {}, {} controlled blocks, text keys {}; {}",
                s.start,
                s.stop,
                s.cycle,
                s.frequency,
                s.blocks.len(),
                s.text_keys,
                if travel.is_empty() { "no node translated by more than one unit".into() } else { travel.join("; ") }
            );
        }
        let _ = writeln!(
            out,
            "  nodes a sequence translates by more than one unit: {}",
            moved.len()
        );
        // Collision readiness of the reference as a placed instance.
        let mut loader = fo4_scene::Loader::new(
            &store,
            &vfs,
            &m.language,
            fo4_scene::RuleLevel::Hypotheses,
            &b,
        );
        match loader.instance(ek, &b) {
            Ok(i) => {
                let codes: BTreeSet<&str> = i.blockers.iter().map(|e| e.code.as_ref()).collect();
                let _ = writeln!(
                    out,
                    "  as a placed instance: draw {}; collision {}; blocker codes {:?}",
                    state_label(&i.state),
                    state_label(&i.collision),
                    codes
                );
            }
            Err(e) => {
                let _ = writeln!(out, "  as a placed instance: refused, {}", e.code);
            }
        }
        // Collision-marker boxes inside the reference's object bounds.
        if let (Ok(w), [x0, y0, z0, x1, y1, z1]) = (
            fo4_scene::transform::Transform::euler(
                <[f32; 6]>::try_from(place.as_slice()).unwrap_or([0.; 6]),
                1.,
            ),
            obnd.as_slice(),
        ) {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for k in 0..8 {
                let c = [
                    f64::from(if k & 1 != 0 { *x1 } else { *x0 }),
                    f64::from(if k & 2 != 0 { *y1 } else { *y0 }),
                    f64::from(if k & 4 != 0 { *z1 } else { *z0 }),
                ];
                let p = w.apply(c);
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            let found = fo4_walk::platform::marker_boxes(&store, lo, hi, &b)?;
            for (bi, mb) in found.iter().enumerate() {
                let _ = writeln!(
                    names,
                    "platform {ei} box {bi}: {}",
                    tl::key_text(&mb.reference)
                );
                let _ = writeln!(
                    out,
                    "  collision-marker box {bi} (inside the axis box around its object bounds): centre {:.1} {:.1} {:.1}, half extents {}, rotation about the vertical {:.3}, an enable parent {}, initially disabled {}",
                    mb.placement[0], mb.placement[1], mb.placement[2],
                    nums(&mb.half),
                    mb.placement[5],
                    mb.enable_parent,
                    mb.initially_disabled
                );
            }
            if found.is_empty() {
                let _ = writeln!(
                    out,
                    "  no collision-marker box inside the axis box around its object bounds"
                );
            }
        }
    }

    // 4. Image space.
    let _ = writeln!(out, "\n## 4. Image space");
    let mut spaces: BTreeSet<FormKey> = BTreeSet::new();
    for e in &t.events {
        if let What::Player { space, .. } = &e.what {
            spaces.insert(tl::norm(space));
        }
    }
    let show = |out: &mut String, names: &mut String, what: &str, k: &FormKey| -> Result<(), E> {
        let Some(r) = store.raw(k, &b)? else {
            let _ = writeln!(out, "{what}: no record");
            return Ok(());
        };
        let _ = writeln!(names, "{what}: {} {}", tl::key_text(k), edid(&r));
        let mut line = format!("{what}:");
        for sg in [b"HNAM", b"CNAM", b"TNAM", b"DNAM"] {
            if let Some(d) = sub(&r, sg) {
                let _ = write!(line, " {} [{}]", sig(*sg), nums(&words(d)));
            }
        }
        let _ = write!(line, "; a texture {}", sub(&r, b"TX00").is_some());
        let _ = writeln!(out, "{line}");
        Ok(())
    };
    for (si, s) in spaces.iter().enumerate() {
        let Some((r, p)) = store.raw_with_plugin(s, &b)? else {
            continue;
        };
        if r.signature == *b"CELL" {
            match form(&store, &r, p, b"XCIM", 0) {
                Some(im) => show(&mut out, &mut names, &format!("interior {si} (XCIM)"), &im)?,
                None => {
                    let _ = writeln!(out, "interior {si}: no XCIM");
                }
            }
        } else if r.signature == *b"WRLD" {
            let Some(cl) = form(&store, &r, p, b"CNAM", 0) else {
                continue;
            };
            let Some((cr, cp)) = store.raw_with_plugin(&cl, &b)? else {
                continue;
            };
            let Some(wk) = form(&store, &cr, cp, b"WLST", 0) else {
                continue;
            };
            let Some((wr, wp)) = store.raw_with_plugin(&wk, &b)? else {
                continue;
            };
            let slots = sub(&wr, b"IMSP").map_or(0, |d| d.len() / 4);
            for slot in 0..slots {
                if let Some(im) = form(&store, &wr, wp, b"IMSP", slot * 4) {
                    show(
                        &mut out,
                        &mut names,
                        &format!(
                            "worldspace {si}, its climate's first weather, slot {slot} (IMSP)"
                        ),
                        &im,
                    )?;
                }
            }
        }
    }
    // Ranges over the load order.
    let all: Vec<FormKey> = store
        .winners()
        .filter(|w| w.signature == *b"IMGS")
        .map(|w| store.overrides().form_key(w.key, store.order()))
        .collect();
    let mut ranges: BTreeMap<(String, usize), (f32, f32, BTreeSet<u32>)> = BTreeMap::new();
    for k in &all {
        let Some(r) = store.raw(k, &b)? else { continue };
        for sg in [b"HNAM", b"CNAM", b"TNAM"] {
            if let Some(d) = sub(&r, sg) {
                for (i, v) in words(d).into_iter().enumerate() {
                    let e = ranges.entry((sig(*sg), i)).or_insert((
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        BTreeSet::new(),
                    ));
                    e.0 = e.0.min(v);
                    e.1 = e.1.max(v);
                    e.2.insert(v.to_bits());
                }
            }
        }
    }
    let _ = writeln!(
        out,
        "IMGS records: {}; per stored word: minimum, maximum, distinct values",
        all.len()
    );
    for ((s, i), (lo, hi, d)) in &ranges {
        let _ = writeln!(out, "  {s} word {i}: {lo} to {hi}, {} distinct", d.len());
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
