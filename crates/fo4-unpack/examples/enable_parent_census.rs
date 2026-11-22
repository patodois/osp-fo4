#![forbid(unsafe_code)]
//! Census of enable parents (XESP) over every winning placed record of the
//! load order (`spec/enable-parent.md`): counts only, written to `OUT.txt`.
//!
//! `enable_parent_census MANIFEST OUT.txt`
//!
//! For every placed record: its type, the group that holds it, the header's
//! initially-disabled flag; for each XESP its size, the flag byte and the
//! three bytes after it, whether the FormID resolves, the parent's record
//! type, group, cell and flag, the chain depth and how the chain ends
//! (`fo4_plugin::enable::chain`), and the initial state each pending rule
//! of `spec/hypotheses.tsv` would give. No name, FormID or path is written.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    enable::{self, ChainEnd},
    typed::Store,
    FormKey,
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}
fn group(kind: Option<u32>) -> &'static str {
    match kind {
        Some(8) => "persistent group (8)",
        Some(9) => "temporary group (9)",
        Some(10) => "distant group (10)",
        Some(_) => "other group",
        None => "not a cell child",
    }
}
fn bucket(n: u64) -> &'static str {
    match n {
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        20..=49 => "20-49",
        50..=99 => "50-99",
        _ => "100+",
    }
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("enable_parent_census MANIFEST OUT.txt".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let order = store.order();
    let mut c = Counts::new();

    // Where each placed record lives: interned cell plugin, cell object id, group kind.
    let mut plugins: Vec<String> = Vec::new();
    let mut place: HashMap<(u32, u32), (u32, u32, u32)> = HashMap::new();
    let intern = |plugins: &mut Vec<String>, p: &str| -> u32 {
        match plugins.iter().position(|x| x == p) {
            Some(i) => i as u32,
            None => {
                plugins.push(p.to_string());
                (plugins.len() - 1) as u32
            }
        }
    };
    let mut placed: Vec<(FormKey, [u8; 4], bool)> = Vec::new();
    for (o, cell, kind) in store.placement_groups() {
        let k = store.overrides().form_key(o.key, order);
        let kp = intern(&mut plugins, &k.plugin);
        let cp = intern(&mut plugins, &cell.plugin);
        place.insert((kp, k.object_id), (cp, cell.object_id, kind));
        if o.deleted {
            bump(
                &mut c,
                format!("placed {}: deleted (not read)", sig(o.signature)),
            );
            continue;
        }
        placed.push((k, o.signature, o.disabled));
    }
    let where_is = |plugins: &Vec<String>, k: &FormKey| -> Option<(u32, u32, u32)> {
        let p = plugins.iter().position(|x| *x == k.plugin)? as u32;
        place.get(&(p, k.object_id)).copied()
    };

    let mut parents: HashMap<FormKey, u64> = HashMap::new();
    let mut with_xesp = 0u64;
    for (k, s, disabled) in &placed {
        let s = sig(*s);
        bump(&mut c, format!("placed {s}"));
        if *disabled {
            bump(&mut c, format!("placed {s}: header flag 0x800"));
        }
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            bump(&mut c, "placed: no winning record (index mismatch)");
            continue;
        };
        let n = r
            .subrecords
            .iter()
            .filter(|x| x.signature == enable::XESP)
            .count();
        if n == 0 {
            continue;
        }
        with_xesp += 1;
        bump(&mut c, format!("xesp: placed {s} with XESP"));
        if *disabled {
            bump(
                &mut c,
                format!("xesp: placed {s} with XESP and header flag 0x800"),
            );
        }
        bump(&mut c, format!("xesp: XESP per record {n}"));
        for x in r.subrecords.iter().filter(|x| x.signature == enable::XESP) {
            bump(&mut c, format!("xesp: size {}", r.data(x).len()));
        }
        let me = where_is(&plugins, k);
        bump(&mut c, format!("xesp: child in {}", group(me.map(|v| v.2))));
        let link = match enable::read(&r, plugin, &store) {
            Ok(Some(l)) => l,
            Ok(None) => continue,
            Err(e) => {
                bump(&mut c, format!("xesp: read error {}", e.code));
                continue;
            }
        };
        bump(&mut c, format!("xesp: flag byte 0x{:02x}", link.flags));
        bump(
            &mut c,
            format!(
                "xesp: bytes 5..7 {}",
                if link.reserved == [0; 3] {
                    "all zero"
                } else {
                    "not all zero"
                }
            ),
        );
        if link.reserved != [0; 3] {
            bump(
                &mut c,
                format!(
                    "xesp: bytes 5..7 not all zero: {:02x} {:02x} {:02x}, flag byte 0x{:02x}",
                    link.reserved[0], link.reserved[1], link.reserved[2], link.flags
                ),
            );
        }
        let Some(pk) = link.parent.clone() else {
            bump(
                &mut c,
                if link.raw == 0 {
                    "xesp: parent FormID 0"
                } else {
                    "xesp: parent FormID unresolved by the load order"
                },
            );
            continue;
        };
        *parents.entry(pk.clone()).or_default() += 1;
        let Some(ps) = store.signature(&pk) else {
            bump(&mut c, "xesp: parent names no winning record");
            continue;
        };
        bump(&mut c, format!("xesp: parent record type {}", sig(ps)));
        let pw = store.overrides().winner(&pk);
        if pw.is_some_and(|w| w.deleted) {
            bump(&mut c, "xesp: parent deleted");
        }
        if pw.is_some_and(|w| w.disabled) {
            bump(&mut c, "xesp: parent header flag 0x800");
        }
        let there = where_is(&plugins, &pk);
        bump(
            &mut c,
            format!("xesp: parent in {}", group(there.map(|v| v.2))),
        );
        bump(
            &mut c,
            match (me, there) {
                (_, None) => "xesp: parent cell: parent not a cell child",
                (None, _) => "xesp: parent cell: child not a cell child",
                (Some(a), Some(p)) if (a.0, a.1) == (p.0, p.1) => "xesp: parent cell: same cell",
                (Some(_), Some(_)) => "xesp: parent cell: another cell",
            },
        );
        if let (Some(a), Some(p)) = (me, there) {
            if (a.0, a.1) != (p.0, p.1) {
                let kind = |v: (u32, u32, u32)| {
                    store.kind(&FormKey {
                        plugin: plugins[v.0 as usize].clone(),
                        object_id: v.1,
                    })
                };
                bump(
                    &mut c,
                    format!(
                        "xesp: another cell: child {:?}, parent {:?}, parent {}",
                        kind(a),
                        kind(p),
                        group(Some(p.2))
                    ),
                );
            }
        }
        // The chain and the initial state under each reading.
        let b = Budget::new(m.limits.max_memory);
        let chain = match enable::chain(&store, k, &b) {
            Ok(Some(ch)) => ch,
            Ok(None) => continue,
            Err(e) => {
                bump(&mut c, format!("chain: error {}", e.code));
                continue;
            }
        };
        let end = match &chain.end {
            ChainEnd::Root => "root (no XESP)",
            ChainEnd::NullParent => "FormID 0",
            ChainEnd::UnresolvedParent { .. } => "unresolved FormID",
            ChainEnd::MissingParent(_) => "no winning record",
            ChainEnd::Cycle(_) => "cycle",
            ChainEnd::TooDeep => "too deep",
        };
        bump(&mut c, format!("chain: ends at {end}"));
        bump(&mut c, format!("chain: depth {}", chain.depth()));
        if chain.end != ChainEnd::Root {
            continue;
        }
        let top = chain.nodes.last().expect("a node");
        bump(
            &mut c,
            format!("chain: root record type {}", sig(top.signature)),
        );
        if top.deleted {
            bump(&mut c, "chain: root deleted");
        }
        // Rule readings: the root's state is its own flag; each link keeps
        // or inverts by flag bit 0; the child's own flag ignored or ANDed.
        let mut on = !top.initially_disabled;
        for n in chain.nodes.iter().rev().skip(1) {
            if n.link.as_ref().is_some_and(|l| l.flags & 1 != 0) {
                on = !on;
            }
        }
        let bits: u8 = chain
            .nodes
            .iter()
            .filter_map(|n| n.link.as_ref())
            .fold(0, |a, l| a | l.flags);
        bump(
            &mut c,
            format!("chain: flag bits used along the chain 0x{bits:02x}"),
        );
        bump(
            &mut c,
            format!(
                "state: chain says {}, child header flag 0x800 {}",
                if on { "enabled" } else { "disabled" },
                if *disabled { "set" } else { "clear" }
            ),
        );
        bump(
            &mut c,
            format!(
                "state: chain says {}, direct parent opposite flag {}, root flag 0x800 {}",
                if on { "enabled" } else { "disabled" },
                u8::from(link.flags & 1 != 0),
                if top.initially_disabled {
                    "set"
                } else {
                    "clear"
                }
            ),
        );
    }
    bump(&mut c, format!("xesp: distinct parents {}", parents.len()));
    for n in parents.values() {
        bump(&mut c, format!("xesp: children per parent {}", bucket(*n)));
    }
    // Parents that are children too.
    let mut middle = 0u64;
    for p in parents.keys() {
        let b = Budget::new(m.limits.max_memory);
        if let Some((r, _)) = store.raw_with_plugin(p, &b)? {
            if r.subrecords.iter().any(|x| x.signature == enable::XESP) {
                middle += 1;
            }
        }
    }
    bump(
        &mut c,
        format!("xesp: distinct parents that carry XESP {middle}"),
    );

    let mut out = String::new();
    writeln!(
        out,
        "# enable_parent_census: XESP of every winning placed record (spec/enable-parent.md)"
    )?;
    writeln!(
        out,
        "# Counts only. chain: fo4_plugin::enable::chain from each placed record with XESP."
    )?;
    writeln!(
        out,
        "# state: the readings of the pending rules (root's own header flag 0x800; flag bit 0 inverts)."
    )?;
    writeln!(
        out,
        "placed records read {}; with XESP {with_xesp}",
        placed.len()
    )?;
    for (k, v) in &c {
        writeln!(out, "{v}\t{k}")?;
    }
    std::fs::write(&args[1], out)?;
    eprintln!("done in {:.0} s", started.elapsed().as_secs_f64());
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
