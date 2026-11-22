#![forbid(unsafe_code)]
//! The fills of reference aliases that a run's readings leave empty, and
//! what else in the records could fill them (`spec/vm.md`, Alias fills
//! from repeated and placed rows): `alias_fill_census MANIFEST OUT.txt NAMES.txt
//! [--player-base EDITOR_ID]`.
//!
//! OUT.txt holds counts and kinds only (`spec/alias-fill-census.txt`);
//! NAMES.txt names quests, aliases and records and is never versioned.
//! Nothing runs:
//!
//! 1. every reference alias of every winning quest by how it is filled and
//!    what the readings that exist give it (a forced reference; a unique
//!    actor's one placed actor or the player; a location's special
//!    reference, `quest_alias_location_ref_v1`), or why it stays empty;
//! 2. the aliases filled from a location alias with a location ref type
//!    whose location is forced: the placed references that carry the ref
//!    type in their own `XLRT` and are tied to the location, by three
//!    ties read apart (the reference's own `XLCN`, its cell's `XLCN`, its
//!    cell's location or one of that location's parents through `PNAM`),
//!    counted none, one or several, against what the special reference
//!    list gives (so the aliases the list resolves test the reading);
//! 3. the placed references that carry an `XLRT` at all: words per
//!    subrecord, with or without their own `XLCN`, record type;
//! 4. the alias flag words (FNAM) of the aliases of section 2 the list
//!    does not resolve to one row, bit by bit as stored.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    quest::{AliasKind, AliasValue, Quest},
    typed::Store,
    FormKey,
};
use fo4_vm::bind::{self, record_key};
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

/// A placed reference carrying an `XLRT`: its ref types, its own `XLCN`
/// and its cell.
struct Tagged {
    key: FormKey,
    types: Vec<FormKey>,
    own_location: Option<FormKey>,
    cell: FormKey,
}

fn words(d: &[u8]) -> Vec<u32> {
    d.as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect()
}

fn several(n: usize) -> &'static str {
    match n {
        0 => "none",
        1 => "one",
        _ => "several",
    }
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut player_base = None;
    if let Some(i) = a.iter().position(|x| x == "--player-base") {
        player_base = Some(a.get(i + 1).cloned().ok_or("--player-base EDITOR_ID")?);
        a.drain(i..i + 2);
    }
    if a.len() != 3 {
        return Err(
            "alias_fill_census MANIFEST OUT.txt NAMES.txt [--player-base EDITOR_ID]".into(),
        );
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let max = m.limits.max_memory;
    let b = Budget::new(max);
    let mut c = Counts::new();
    let mut names = String::new();
    let winners = |sig: &[u8; 4]| -> Vec<FormKey> {
        store
            .winners()
            .filter(|w| !w.deleted && w.signature == *sig)
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect()
    };
    let remap = |plugin: usize, id: u32, file: &str, o: u64| {
        store.order().remap(plugin, id, file, o).ok().flatten()
    };
    // The player's base, as the caller names it.
    let player = match &player_base {
        Some(e) => winners(b"NPC_").into_iter().find(|k| {
            store
                .raw(k, &b.child())
                .ok()
                .flatten()
                .and_then(|r| bind::editor_id(&r))
                .is_some_and(|x| x.eq_ignore_ascii_case(e))
        }),
        None => None,
    };
    // Locations: their parent (PNAM).
    let mut parent: BTreeMap<FormKey, FormKey> = BTreeMap::new();
    for k in winners(b"LCTN") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        if let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"PNAM") {
            if let Some(w) = words(r.data(s)).first() {
                if let Some(p) = remap(plugin, *w, &r.source.file, s.offset) {
                    parent.insert(k, p);
                }
            }
        }
    }
    let within = |mut l: FormKey, want: &FormKey| -> bool {
        for _ in 0..32 {
            if &l == want {
                return true;
            }
            match parent.get(&l) {
                Some(p) => l = p.clone(),
                None => return false,
            }
        }
        false
    };
    // Cells: their location (XLCN).
    let mut cell_location: BTreeMap<FormKey, FormKey> = BTreeMap::new();
    for k in winners(b"CELL") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        if let Some(s) = r.subrecords.iter().find(|s| s.signature == *b"XLCN") {
            if let Some(w) = words(r.data(s)).first() {
                if let Some(l) = remap(plugin, *w, &r.source.file, s.offset) {
                    cell_location.insert(k, l);
                }
            }
        }
    }
    // 3: placed references carrying an XLRT.
    let mut tagged: Vec<Tagged> = Vec::new();
    let mut placed_actors: BTreeMap<String, usize> = BTreeMap::new();
    for (o, cell) in store.placement_cells() {
        if o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        if o.signature == *b"ACHR" {
            if let Ok(Some(base)) = bind::base_of(&store, &k, &b.child()) {
                *placed_actors.entry(record_key(&base)).or_default() += 1;
            }
        }
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let x: Vec<_> = r
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"XLRT")
            .collect();
        if x.is_empty() {
            continue;
        }
        let sig = String::from_utf8_lossy(&r.signature).into_owned();
        let mut types = Vec::new();
        for s in &x {
            let d = r.data(s);
            bump(
                &mut c,
                format!(
                    "3 XLRT subrecords by length\t{} bytes{}",
                    d.len(),
                    if d.len() % 4 == 0 { "" } else { " (not words)" }
                ),
            );
            for w in words(d) {
                match remap(plugin, w, &r.source.file, s.offset) {
                    Some(t) if store.signature(&t) == Some(*b"LCRT") => types.push(t),
                    Some(_) => bump(&mut c, "3 XLRT words naming another record type"),
                    None => bump(&mut c, "3 XLRT words naming no record"),
                }
            }
        }
        let own_location = r
            .subrecords
            .iter()
            .find(|s| s.signature == *b"XLCN")
            .and_then(|s| {
                words(r.data(s))
                    .first()
                    .and_then(|w| remap(plugin, *w, &r.source.file, s.offset))
            });
        bump(
            &mut c,
            format!(
                "3 placed references carrying XLRT\t{sig}\t{}",
                if own_location.is_some() {
                    "with their own XLCN"
                } else {
                    "without their own XLCN"
                }
            ),
        );
        if let Some(l) = &own_location {
            let tie = match cell_location.get(&cell) {
                Some(cl) if cl == l => "their own XLCN is their cell's location",
                Some(cl) if within(cl.clone(), l) => {
                    "their own XLCN is a parent of their cell's location"
                }
                Some(_) => "their own XLCN is another location than their cell's",
                None => "their cell has no location",
            };
            bump(
                &mut c,
                format!("3 placed references carrying XLRT and XLCN\t{sig}\t{tie}"),
            );
        }
        tagged.push(Tagged {
            key: k,
            types,
            own_location,
            cell,
        });
    }
    let mut by_type: BTreeMap<FormKey, Vec<usize>> = BTreeMap::new();
    for (i, t) in tagged.iter().enumerate() {
        for ty in &t.types {
            by_type.entry(ty.clone()).or_default().push(i);
        }
    }
    // 1, 2, 4: the aliases.
    for k in winners(b"QUST") {
        let bb = b.child();
        let Some((r, plugin)) = store.raw_with_plugin(&k, &bb)? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let Ok(q) = Quest::read(&r, ctx, &bb) else {
            bump(&mut c, "1 quests that do not read");
            continue;
        };
        let forced_location = |id: i32| -> Option<FormKey> {
            q.aliases
                .iter()
                .find(|x| x.kind == AliasKind::Location && i64::from(x.id) == i64::from(id))
                .and_then(|x| {
                    x.values.iter().find_map(|v| match v {
                        AliasValue::ForcedLocation(f) => f.key.clone(),
                        _ => None,
                    })
                })
        };
        let location_kind = |id: i32| -> &'static str {
            let Some(x) = q
                .aliases
                .iter()
                .find(|x| x.kind == AliasKind::Location && i64::from(x.id) == i64::from(id))
            else {
                return "the location alias is not in the quest";
            };
            let tags: BTreeSet<[u8; 4]> = x.values.iter().map(|v| v.tag()).collect();
            if tags.contains(b"ALFL") {
                "a forced location"
            } else if tags.contains(b"ALFE") {
                "the story event's location"
            } else if tags.contains(b"ALFA") {
                "another alias's location"
            } else if tags.contains(b"ALEQ") {
                "an alias of another quest"
            } else if tags.contains(b"CTDA") {
                "by conditions only"
            } else {
                "no fill value"
            }
        };
        for al in q.aliases.iter().filter(|x| x.kind != AliasKind::Location) {
            let kind = match al.kind {
                AliasKind::Collection { .. } => "collection alias",
                _ => "reference alias",
            };
            let tags: BTreeSet<[u8; 4]> = al.values.iter().map(|v| v.tag()).collect();
            let has = |t: &[u8; 4]| tags.contains(t);
            let label = format!(
                "alias {} {:?} of QUST {}",
                al.id,
                String::from_utf8_lossy(&al.name),
                record_key(&k)
            );
            let what: String = if has(b"ALFR") {
                "a forced reference (ALFR): filled".into()
            } else if has(b"ALUA") {
                let u = al.values.iter().find_map(|v| match v {
                    AliasValue::UniqueActor(f) => Some(f.key.clone()),
                    _ => None,
                });
                match u.flatten() {
                    None => "a unique actor (ALUA) naming no record: empty".into(),
                    Some(n) if Some(&n) == player.as_ref() => {
                        "a unique actor (ALUA), the player's base: the player".into()
                    }
                    Some(n) => match placed_actors.get(&record_key(&n)).copied().unwrap_or(0) {
                        1 => "a unique actor (ALUA), one placed actor: filled".into(),
                        0 => "a unique actor (ALUA), no placed actor: empty".into(),
                        _ => "a unique actor (ALUA), several placed actors: empty".into(),
                    },
                }
            } else if has(b"ALFA") {
                let (l, t) = al
                    .values
                    .iter()
                    .find_map(|v| match v {
                        AliasValue::LocationAliasReference { alias, ref_type } => {
                            Some((*alias, ref_type.as_ref().and_then(|t| t.key.clone())))
                        }
                        _ => None,
                    })
                    .expect("ALFA");
                let lk = location_kind(l);
                match (t, forced_location(l)) {
                    (None, _) => format!(
                        "from a location alias (ALFA) with no ref type, the location {lk}: empty"
                    ),
                    (Some(_), None) => format!(
                        "from a location alias (ALFA) with a ref type, the location {lk}: empty"
                    ),
                    (Some(t), Some(loc)) => {
                        let rows = fo4_vm::quest::special_references(&store, &loc, &bb)?;
                        let listed: Vec<FormKey> = rows
                            .as_ref()
                            .map(|rows| {
                                rows.iter()
                                    .filter(|(rt, _)| rt.as_ref() == Some(&t))
                                    .filter_map(|(_, r)| r.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut distinct: Vec<FormKey> = Vec::new();
                        for x in &listed {
                            if !distinct.contains(x) {
                                distinct.push(x.clone());
                            }
                        }
                        let lcsr = match (&rows, listed.len(), distinct.len()) {
                            (None, _, _) => "the special references do not read",
                            (Some(_), 0, _) => "no special reference of the type",
                            (Some(_), 1, _) => "one special reference of the type",
                            (Some(_), _, 1) => "several rows of the type, all naming one reference",
                            (Some(_), _, _) => "several distinct references of the type",
                        };
                        // Section 2: the placed references carrying the type.
                        let cands: Vec<&Tagged> = by_type
                            .get(&t)
                            .map(|v| v.iter().map(|i| &tagged[*i]).collect())
                            .unwrap_or_default();
                        let own: Vec<&FormKey> = cands
                            .iter()
                            .filter(|x| x.own_location.as_ref() == Some(&loc))
                            .map(|x| &x.key)
                            .collect();
                        let cell: Vec<&FormKey> = cands
                            .iter()
                            .filter(|x| cell_location.get(&x.cell) == Some(&loc))
                            .map(|x| &x.key)
                            .collect();
                        let below: Vec<&FormKey> = cands
                            .iter()
                            .filter(|x| {
                                cell_location
                                    .get(&x.cell)
                                    .is_some_and(|cl| within(cl.clone(), &loc))
                            })
                            .map(|x| &x.key)
                            .collect();
                        for (tie, v) in [
                            ("their own XLCN is the location", &own),
                            ("their cell's XLCN is the location", &cell),
                            ("their cell's location is the location or below it", &below),
                        ] {
                            let agree = match (distinct.as_slice(), v.as_slice()) {
                                ([one], [x]) if one == *x => ", the same reference as the list",
                                ([_], [_]) => ", another reference than the list",
                                _ => "",
                            };
                            bump(
                                &mut c,
                                format!(
                                    "2 placed references carrying the type\t{lcsr}\t{tie}\t{}{agree}",
                                    several(v.len())
                                ),
                            );
                        }
                        if distinct.len() != 1 {
                            let fnam = al.flags;
                            for bit in 0..32 {
                                if fnam & (1 << bit) != 0 {
                                    bump(
                                        &mut c,
                                        format!("4 flag bits of the aliases the list does not resolve to one reference\t{lcsr}\tbit 0x{:x}", 1u32 << bit),
                                    );
                                }
                            }
                            if fnam == 0 {
                                bump(
                                    &mut c,
                                    format!("4 flag bits of the aliases the list does not resolve to one reference\t{lcsr}\tno bit"),
                                );
                            }
                            let _ = writeln!(
                                names,
                                "## {label}: {lcsr} (LCRT {} in LCTN {}); own XLCN {:?}; cell XLCN {:?}; cell location or below {:?}; FNAM {fnam:#x}",
                                record_key(&t),
                                record_key(&loc),
                                own.iter().map(|k| record_key(k)).collect::<Vec<_>>(),
                                cell.iter().map(|k| record_key(k)).collect::<Vec<_>>(),
                                below.iter().map(|k| record_key(k)).collect::<Vec<_>>(),
                            );
                        }
                        format!(
                            "from a location alias (ALFA) with a ref type, a forced location: {lcsr}{}",
                            match (listed.len(), distinct.len()) {
                                (1, _) => ": filled",
                                (_, 1) => ": empty (one reference under the distinct-rows reading)",
                                _ => ": empty",
                            }
                        )
                    }
                }
            } else if has(b"ALFE") {
                "from the story event (ALFE): empty".into()
            } else if has(b"ALCO") {
                "created at run time (ALCO): empty".into()
            } else if has(b"ALEQ") {
                "an alias of another quest (ALEQ): empty".into()
            } else if has(b"ALNA") {
                "linked to another alias (ALNA): empty".into()
            } else if has(b"ALDN") {
                "a display name only (ALDN): empty".into()
            } else if has(b"CTDA") {
                "by conditions only (CTDA): empty".into()
            } else {
                "no fill value: empty".into()
            };
            bump(&mut c, format!("1 aliases by fill\t{kind}\t{what}"));
            let _ = writeln!(names, "# {label}: {what}");
        }
    }
    let mut out = String::from(
        "# alias_fill_census: kinds and counts only (spec/vm.md, Alias fills from repeated and placed rows). Sections: 1 reference and collection aliases of every winning quest by fill and what the existing readings give;\n# 2 aliases filled from a forced location by a ref type: placed references carrying the type, by how they are tied to the location, against the special reference list; 3 placed references carrying XLRT; 4 flag bits of the aliases the list does not resolve to one row.\n",
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
