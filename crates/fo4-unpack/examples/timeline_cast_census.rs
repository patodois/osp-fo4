#![forbid(unsafe_code)]
//! Census of the actors a run timeline places (`spec/timeline.md`, `spec/timeline-cast-census.txt`):
//! why a frame draws each one, leaves it out as disabled or cannot decide its
//! presence, by reason, and what each reading of the enable chain would give.
//! Counts and descriptions only in `OUT.txt`; record keys and editor ids only
//! in `NAMES.txt` (never versioned).
//!
//! `timeline_cast_census MANIFEST TIMELINE OUT.txt NAMES.txt [--at S,S,...]`
//!
//! Presence is decided as a frame decides it (`fo4_scene::timeline::presence`):
//! the actor's own last Enable or Disable in the run, else its enable parent
//! chain under the enable parent rules with the run's changes, else its own
//! initially-disabled flag. The chain's structure is static, so the reasons
//! are counted once per actor; the presence counts are given at each second
//! of `--at` (default: the run's last second) with every reading off, with
//! the NoWait forms read (`timeline_nowait_visibility_v1`), with
//! `xesp_child_flag_agrees_chain_v1`, and with both.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{enable::ChainEnd, typed::Store, FormKey};
use fo4_scene::timeline::{self as tl, Presence, Readings, Spot, Timeline, What};
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
fn sig(s: [u8; 4]) -> String {
    String::from_utf8_lossy(&s).into_owned()
}
fn presence_text(p: &Presence) -> String {
    match p {
        Presence::Run(true) => "present: its own Enable in the run".into(),
        Presence::Run(false) => "absent: its own Disable in the run".into(),
        Presence::Chain { present: true, .. } => "present: its enable chain".into(),
        Presence::Chain { present: false, .. } => "absent: its enable chain".into(),
        Presence::Flag(true) => "present: no enable parent, flag 0x800 clear".into(),
        Presence::Flag(false) => "absent: no enable parent, flag 0x800 set".into(),
        Presence::Undecided(why) => format!("undecided: {}", why.join(" + ")),
        Presence::NoRecord => "no winning record".into(),
    }
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    const USAGE: &str = "timeline_cast_census MANIFEST TIMELINE OUT.txt NAMES.txt [--at S,S,...]";
    if args.len() != 4 && args.len() != 6 {
        return Err(USAGE.into());
    }
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let t = Timeline::parse(&std::fs::read_to_string(&args[1])?)?;
    let last = t.events.last().map_or(0., |e| e.t);
    let seconds: Vec<f64> = if args.len() == 6 {
        if args[4] != "--at" {
            return Err(USAGE.into());
        }
        args[5]
            .split(',')
            .map(|s| s.parse::<f64>())
            .collect::<Result<_, _>>()?
    } else {
        vec![last]
    };
    let mut out = String::new();
    let mut names = String::new();
    let _ = writeln!(
        out,
        "# Actors a run timeline places, by how a frame decides their presence (timeline_cast_census; counts only)"
    );

    // Every actor the timeline places, the sources and spaces of its places.
    let mut actors: BTreeMap<FormKey, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    let mut own_changes: BTreeMap<FormKey, u64> = BTreeMap::new();
    let mut changed: BTreeSet<FormKey> = BTreeSet::new();
    for e in &t.events {
        match &e.what {
            What::Actor {
                reference,
                at,
                source,
            } => {
                let entry = actors.entry(tl::norm(reference)).or_default();
                entry.0.insert(source.clone());
                if let Spot::At { space, .. } = at {
                    let kind = match store.signature(space).map(sig).as_deref() {
                        Some("CELL") => "interior cell",
                        Some("WRLD") => "worldspace",
                        _ => "other space",
                    };
                    entry.1.insert(kind.into());
                } else {
                    entry.1.insert("place left open".into());
                }
            }
            What::Visibility { reference, .. } => {
                changed.insert(tl::norm(reference));
                *own_changes.entry(tl::norm(reference)).or_default() += 1;
            }
            _ => {}
        }
    }
    let _ = writeln!(out, "actors placed by the timeline\t{}", actors.len());

    // 1. Static reasons, once per actor.
    let mut reasons = Counts::new();
    let mut detail = Counts::new();
    let mut parents: BTreeMap<FormKey, u64> = BTreeMap::new();
    for (k, (sources, spaces)) in &actors {
        let label = t.labels.get(k).cloned().unwrap_or_default();
        let chain = fo4_plugin::enable::chain(&store, k, &b)?;
        let own = own_changes.get(k).copied().unwrap_or(0);
        let class = match &chain {
            None => "no winning record".to_string(),
            Some(c) if c.nodes.len() == 1 => {
                if c.nodes[0].initially_disabled {
                    "no enable parent, flag 0x800 set".into()
                } else {
                    "no enable parent, flag 0x800 clear".into()
                }
            }
            Some(c) => {
                let f = fo4_scene::enable::findings(c);
                let open: Vec<&str> = f
                    .iter()
                    .copied()
                    .filter(|x| fo4_scene::enable::rule(x).is_none())
                    .collect();
                if open.is_empty() {
                    "enable chain decided by the enable parent rules".into()
                } else {
                    format!("enable chain undecided: {}", open.join(" + "))
                }
            }
        };
        bump(&mut reasons, class.clone());
        bump(
            &mut detail,
            format!(
                "{class}\twhere placed: {}",
                spaces.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        );
        bump(
            &mut detail,
            format!(
                "{class}\tsources: {}",
                sources.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        );
        bump(
            &mut detail,
            format!(
                "{class}\tthe actor's own Enable/Disable in the run: {}",
                if own > 0 { "yes" } else { "no" }
            ),
        );
        let mut chain_note = String::new();
        if let Some(c) = chain.as_ref().filter(|c| c.nodes.len() > 1) {
            let n = c.nodes.len();
            let flagged: Vec<usize> = (0..n.saturating_sub(1))
                .filter(|i| c.nodes[*i].initially_disabled)
                .collect();
            let root = &c.nodes[n - 1];
            let bits = c
                .nodes
                .iter()
                .filter_map(|x| x.link.as_ref())
                .fold(0u8, |a, l| a | l.flags);
            let end = match &c.end {
                ChainEnd::Root => "ends at a record without XESP".to_string(),
                e => format!("ends {e:?}").chars().take(24).collect(),
            };
            let touched: Vec<usize> = (1..n)
                .filter(|i| changed.contains(&tl::norm(&c.nodes[*i].key)))
                .collect();
            let root_base = store
                .get::<fo4_plugin::typed::Refr>(&root.key, &b)
                .ok()
                .flatten()
                .and_then(|r| r.data.base.and_then(|f| f.key))
                .and_then(|k| store.signature(&k))
                .map_or("-".into(), sig);
            for line in [
                format!("depth {}", c.depth()),
                end.clone(),
                format!(
                    "flag 0x800 below the root: {}",
                    match flagged.as_slice() {
                        [] => "none".to_string(),
                        [0] => "on the actor itself".into(),
                        _ if flagged.contains(&0) =>
                            "on the actor and an intermediate record".into(),
                        _ => "on an intermediate record".into(),
                    }
                ),
                format!(
                    "root {} (base {root_base}), flag 0x800 {}",
                    sig(root.signature),
                    if root.initially_disabled {
                        "set"
                    } else {
                        "clear"
                    }
                ),
                format!("link flag bits along the chain: {bits:#04x}"),
                format!(
                    "the run enables or disables a record above the actor: {}",
                    if touched.is_empty() { "no" } else { "yes" }
                ),
                format!(
                    "every own flag 0x800 below the root agrees with the chain's start state: {}",
                    if flagged.is_empty() {
                        "no flag below the root"
                    } else if fo4_scene::enable::child_flags_agree(c) {
                        "yes"
                    } else {
                        "no"
                    }
                ),
            ] {
                bump(&mut detail, format!("{class}\t{line}"));
            }
            *parents.entry(tl::norm(&c.nodes[1].key)).or_default() += 1;
            let _ = write!(
                chain_note,
                "\tparent {} {}\tdepth {}\tbits {bits:#04x}\tflagged below root {flagged:?}\troot flag {}\trun touches {:?}",
                tl::key_text(&c.nodes[1].key),
                t.labels.get(&tl::norm(&c.nodes[1].key)).cloned().unwrap_or_default(),
                c.depth(),
                root.initially_disabled,
                touched
            );
        }
        let _ = writeln!(
            names,
            "{}\t{label}\t{class}\town changes {own}{chain_note}",
            tl::key_text(k)
        );
    }
    let _ = writeln!(
        out,
        "\n## 1. How each actor's presence is decided (once per actor)"
    );
    for (k, v) in &reasons {
        let _ = writeln!(out, "{k}\t{v}");
    }
    let _ = writeln!(out, "\n## 2. By class: where placed, sources, chain shape");
    for (k, v) in &detail {
        let _ = writeln!(out, "{k}\t{v}");
    }
    let mut sizes = Counts::new();
    for n in parents.values() {
        bump(&mut sizes, format!("parents naming {n} of these actors"));
    }
    let _ = writeln!(out, "\n## 3. Distinct first parents\t{}", parents.len());
    for (k, v) in &sizes {
        let _ = writeln!(out, "{k}\t{v}");
    }

    // 4. Presence at each second, every reading off and with the child flag
    // reading, in the space the player is in and in all spaces.
    let _ = writeln!(
        out,
        "\n## 4. Presence at a second (every actor the timeline places by then)"
    );
    for s in &seconds {
        for (name, nowait, child_flag_agrees) in [
            ("readings off", false, false),
            ("NoWait read", true, false),
            ("child flag agreeing read", false, true),
            ("NoWait and child flag agreeing read", true, true),
        ] {
            let readings = Readings {
                nowait,
                child_flag_agrees,
                ..Readings::default()
            };
            let f = t.at_with(*s, readings);
            let player_space = f.player.as_ref().map(|p| p.space.clone());
            let mut c = Counts::new();
            for (k, (_, at, _)) in &f.actors {
                let here = match at {
                    Spot::At { space, .. } if Some(tl::norm(space)) == player_space => {
                        "the player's space"
                    }
                    Spot::At { .. } => "another space",
                    Spot::Open(_) => "place left open",
                };
                let p = tl::presence(&store, &f, k, &b)?;
                bump(&mut c, format!("{here}\t{}", presence_text(&p)));
            }
            for (k, v) in &c {
                let _ = writeln!(out, "second {s}\t{name}\t{k}\t{v}");
            }
        }
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
