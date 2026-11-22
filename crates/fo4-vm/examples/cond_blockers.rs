#![forbid(unsafe_code)]
//! Census of the condition function indexes that block the condition lists
//! a run evaluates (`spec/conditions.md`, Blockers): scene phase start and
//! completion conditions, quest stage log entry conditions, quest alias
//! fill conditions, a quest's story-event conditions (after NEXT) and
//! story manager node conditions, each evaluated by `fo4_vm::cond` in the
//! new-game state before any script runs (`cond::StaticFacts`).
//!
//! ```text
//! cond_blockers MANIFEST OUT.txt [NAMES.txt] [--no-cond-fn-N]... [--no-cond-run-on]
//! ```
//!
//! A list left undetermined is **blocked only by** a function index when
//! every condition that leaves it open is of that index and the index is
//! not evaluated: evaluating that one index (whatever it answers) would
//! decide the list. Lists also left open by an evaluated function whose
//! inputs the state does not settle are counted apart. OUT.txt holds
//! counts, record type tags and function indexes only; NAMES.txt (never
//! committed) the lists with editor IDs, for the indexes the census details.
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    condition::{self, Comparison},
    scen::Scene,
    story::Tree,
    typed::Store,
};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind::{self, VfsLibrary},
    blockers::{self, Blockers, ListAt},
    cond::{self, Combine, StaticFacts, Switches},
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
fn tag(t: &[u8; 4]) -> String {
    String::from_utf8_lossy(t).into_owned()
}

/// Indexes always detailed, whatever their rank.
const ALWAYS: [u16; 4] = [555, 1, 629, 14];
/// How many of the top-ranked indexes are detailed.
const TOP: usize = 12;

/// What a stored word names when read as a FormID.
fn kind_of(store: &Store, ctx: Context<'_>, raw: u32, file: &str) -> String {
    if raw == 0 {
        return "zero".into();
    }
    match condition::form(ctx, raw, file, 0) {
        Err(_) => "not a FormID".into(),
        Ok(None) => "zero".into(),
        Ok(Some(k)) => match store.signature(&k) {
            Some(s) => tag(&s),
            None if raw < 0x800 => "a number below 0x800".into(),
            None => "no winning record".into(),
        },
    }
}

fn value_kind(c: &condition::Ctda) -> String {
    match c.comparison() {
        Comparison::Global(_) => "a global".into(),
        Comparison::Float(0.0) => "0".into(),
        Comparison::Float(1.0) => "1".into(),
        Comparison::Float(x) if x.fract() == 0.0 && x.abs() < 1e6 => "another integer".into(),
        Comparison::Float(_) => "not an integer".into(),
    }
}

#[derive(Default)]
struct Census {
    c: Counts,
    /// (context, function) -> lists blocked only by it.
    sole: BTreeMap<(&'static str, u16), u64>,
    /// (context, function) -> undetermined lists it is among the blockers of.
    among: BTreeMap<(&'static str, u16), u64>,
    /// (context, function) -> conditions.
    uses: BTreeMap<(&'static str, u16), u64>,
    /// Detail lines per function (context, line) -> count.
    detail: BTreeMap<u16, Counts>,
    /// Names-file lines per function.
    names: BTreeMap<u16, Vec<String>>,
}

fn run() -> Result<(), E> {
    let mut a: Vec<String> = std::env::args().skip(1).collect();
    let mut sw = Switches::all();
    a.retain(|x| {
        if let Some(n) = x
            .strip_prefix("--no-cond-fn-")
            .and_then(|n| n.parse::<u16>().ok())
        {
            sw.off(n);
            false
        } else if x == "--no-cond-run-on" {
            sw.run_on_off = true;
            false
        } else {
            true
        }
    });
    if a.len() < 2 || a.len() > 3 || a.iter().any(|x| x.starts_with("--")) {
        return Err(
            "cond_blockers MANIFEST OUT.txt [NAMES.txt] [--no-cond-fn-N]... [--no-cond-run-on]"
                .into(),
        );
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let b = Budget::new(m.limits.max_memory);
    let mut owned: Vec<ListAt> = Vec::new();
    let mut cs = Census::default();
    for w in store.winners().filter(|w| !w.deleted) {
        let key = store.overrides().form_key(w.key, store.order());
        let bb = Budget::new(m.limits.max_memory);
        let got = match &w.signature {
            b"SCEN" => blockers::of_scene(&store, &key, &bb),
            b"QUST" => blockers::of_quest(&store, &key, &bb),
            _ => continue,
        };
        match got {
            Ok(l) => owned.extend(l),
            Err(_) => bump(
                &mut cs.c,
                format!("records not read\t{}", tag(&w.signature)),
            ),
        }
    }
    let tree = Tree::read(&store, &b)?;
    owned.extend(blockers::of_tree(&tree));
    let items = &owned;

    let mut facts = StaticFacts::new_game(&store, m.limits.max_memory).with_switches(sw.clone());
    if sw.on(629) {
        facts = facts.with_scripts(Box::new(VfsLibrary::new(&vfs, m.limits.max_memory)));
    }
    // First pass: outcomes and blockers.
    let mut open_by_item: Vec<Option<BTreeSet<u16>>> = Vec::with_capacity(items.len());
    for it in items {
        let ctx = Context {
            order: store.order(),
            plugin: it.plugin,
        };
        bump(
            &mut cs.c,
            format!("lists\t{}\t{}", it.context, tag(&it.host)),
        );
        facts.set_owner(it.owner.clone());
        let t = cond::eval_list(&it.list, ctx, &it.file, &mut facts, Combine::OrGroups);
        bump(
            &mut cs.c,
            format!("lists by outcome\t{}\t{}", it.context, t.word()),
        );
        for c in &it.list {
            *cs.uses.entry((it.context, c.ctda.function)).or_default() += 1;
        }
        let Some(bl) = Blockers::of(&t) else {
            open_by_item.push(None);
            continue;
        };
        for f in &bl.not_evaluated {
            *cs.among.entry((it.context, *f)).or_default() += 1;
        }
        for (f, r) in &bl.open {
            bump(
                &mut cs.c,
                format!(
                    "undetermined lists by the reason an evaluated function gives\t{}\t{f}\t{r}",
                    it.context
                ),
            );
        }
        for f in bl.open_functions() {
            bump(
                &mut cs.c,
                format!(
                    "undetermined lists with an evaluated function whose input the state leaves open\t{}\t{f}",
                    it.context
                ),
            );
        }
        bump(
            &mut cs.c,
            format!(
                "undetermined lists by what blocks them\t{}\t{}",
                it.context,
                bl.shape()
            ),
        );
        if let Some(f) = bl.sole() {
            *cs.sole.entry((it.context, f)).or_default() += 1;
        }
        open_by_item.push(Some(bl.not_evaluated));
    }
    // Ranking by lists blocked only by the index, over the six contexts.
    let mut total: BTreeMap<u16, u64> = BTreeMap::new();
    for ((_, f), n) in &cs.sole {
        *total.entry(*f).or_default() += n;
    }
    let mut ranked: Vec<(u16, u64)> = total.into_iter().collect();
    ranked.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
    let mut detailed: BTreeSet<u16> = ranked.iter().take(TOP).map(|x| x.0).collect();
    detailed.extend(ALWAYS);
    detailed.extend(cond::ADDED.iter().map(|x| x.0));

    // Second pass: details of the detailed indexes.
    for (it, open) in items.iter().zip(&open_by_item) {
        let ctx = Context {
            order: store.order(),
            plugin: it.plugin,
        };
        for c in &it.list {
            let f = c.ctda.function;
            if !detailed.contains(&f) {
                continue;
            }
            let d = &c.ctda;
            let dm = cs.detail.entry(f).or_default();
            let x = it.context;
            bump(dm, format!("{x}\trun-on word\t{}", d.run_on));
            if d.run_on == 2 {
                bump(
                    dm,
                    format!(
                        "{x}\treference word (run-on 2)\t{}",
                        kind_of(&store, ctx, d.reference, &it.file)
                    ),
                );
            }
            bump(
                dm,
                format!(
                    "{x}\tparameter 1\t{}",
                    kind_of(&store, ctx, d.param1, &it.file)
                ),
            );
            bump(
                dm,
                format!(
                    "{x}\tparameter 2\t{}",
                    kind_of(&store, ctx, d.param2, &it.file)
                ),
            );
            bump(dm, format!("{x}\toperator code\t{}", d.operator()));
            bump(
                dm,
                format!("{x}\tflag bits other than OR\t0x{:02x}", d.flags() & !0x01),
            );
            bump(dm, format!("{x}\tcomparison value\t{}", value_kind(d)));
            bump(
                dm,
                format!(
                    "{x}\ttexts\t{}",
                    match (&c.cis1, &c.cis2) {
                        (None, None) => "none",
                        (Some(_), None) => "CIS1",
                        (None, Some(_)) => "CIS2",
                        (Some(_), Some(_)) => "CIS1 and CIS2",
                    }
                ),
            );
            if f == 550 {
                // Parameter 2 against the INAM indexes of the scene's actions.
                let bb = Budget::new(m.limits.max_memory);
                let found = condition::form(ctx, d.param1, &it.file, 0)
                    .ok()
                    .flatten()
                    .filter(|k| store.signature(k) == Some(*b"SCEN"))
                    .and_then(|k| store.raw_with_plugin(&k, &bb).ok().flatten())
                    .and_then(|(r, p)| {
                        let sc = Context {
                            order: store.order(),
                            plugin: p,
                        };
                        Scene::read(&r, sc, &bb).ok()
                    })
                    .map(|sc| {
                        let same = Some(&it.key)
                            == condition::form(ctx, d.param1, &it.file, 0)
                                .ok()
                                .flatten()
                                .as_ref();
                        let phase: Option<u32> =
                            it.place.strip_prefix("phase ").and_then(|p| p.parse().ok());
                        (
                            sc.actions.iter().find(|a| a.index == d.param2).map(|a| {
                                let inside =
                                    same && phase.is_some_and(|p| a.start <= p && p <= a.end);
                                (a.kind, inside)
                            }),
                            same,
                        )
                    });
                let what = match found {
                    None => "parameter 1 names no SCEN read".to_string(),
                    Some((Some((k, inside)), same)) => format!(
                        "an action index of {} scene, kind {k}{}",
                        if same { "the list's own" } else { "another" },
                        if inside {
                            ", the action's phases include the condition's phase"
                        } else if same {
                            ", the action ends before the condition's phase or starts after it"
                        } else {
                            ""
                        }
                    ),
                    Some((None, _)) => "no action of the scene has that index".to_string(),
                };
                bump(dm, format!("{x}	parameter 2 against the INAM of the actions of parameter 1's scene	{what}"));
            }
            if let Some(bl) = open {
                if bl.len() == 1 && bl.contains(&f) {
                    bump(dm, format!("{x}\tin a list it alone blocks\tyes"));
                }
            }
            // Names file.
            facts.set_owner(it.owner.clone());
            let one = cond::eval_condition(c, ctx, &it.file, &mut facts, &mut BTreeSet::new());
            let bb = Budget::new(m.limits.max_memory);
            let label = bind::label(&store, &it.key, &bb).unwrap_or_else(|_| "?".into());
            let mut line = format!(
                "{f}\t{x}\t{}\t{label}\t{}\top {} value {:?} p1 {:#x} ({}) p2 {:#x} run-on {} ref {:#x} last {}",
                tag(&it.host),
                it.place,
                d.operator(),
                d.comparison(),
                d.param1,
                kind_of(&store, ctx, d.param1, &it.file),
                d.param2,
                d.run_on,
                d.reference,
                d.param3
            );
            for (n, t) in [("CIS1", &c.cis1), ("CIS2", &c.cis2)] {
                if let Some(t) = t {
                    let _ = write!(line, " {n} {}", String::from_utf8_lossy(t));
                }
            }
            if let Ok(Some(k)) = condition::form(ctx, d.param1, &it.file, 0) {
                if store.signature(&k).is_some() {
                    let _ = write!(
                        line,
                        " p1={}",
                        bind::label(&store, &k, &bb).unwrap_or_default()
                    );
                }
            }
            if d.run_on == 2 {
                if let Ok(Some(k)) = condition::form(ctx, d.reference, &it.file, 0) {
                    let _ = write!(
                        line,
                        " ref={}",
                        bind::label(&store, &k, &bb).unwrap_or_default()
                    );
                }
            }
            let _ = write!(line, " condition {one:?}");
            let _ = write!(
                line,
                " list {}",
                match open {
                    None => "decided".to_string(),
                    Some(bl) => format!("open by {bl:?}"),
                }
            );
            cs.names.entry(f).or_default().push(line);
        }
    }

    let mut out = String::new();
    out.push_str("# cond_blockers: counts, record type tags, contexts and function indexes only (spec/conditions.md, Blockers)\n");
    out.push_str("# Function indexes are bare numbers. New-game state before any script runs (cond::StaticFacts).\n");
    let _ = writeln!(
        out,
        "# switches: {}",
        if sw.is_all() {
            "every evaluator on".to_string()
        } else {
            sw.describe()
        }
    );
    out.push_str("\n## lists and outcomes\n");
    for (k, v) in &cs.c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    out.push_str("\n## ranked: function index, lists blocked only by it over the six contexts\n");
    for (f, n) in &ranked {
        let _ = writeln!(out, "{n}\t{f}");
    }
    out.push_str("\n## by context: function index, lists blocked only by it, undetermined lists it is among the blockers of, conditions of the index in the context\n");
    for x in blockers::CONTEXTS {
        let mut fs: BTreeSet<u16> = BTreeSet::new();
        for (c, f) in cs.uses.keys() {
            if *c == x {
                fs.insert(*f);
            }
        }
        let mut rows: Vec<(u64, u64, u64, u16)> = fs
            .into_iter()
            .map(|f| {
                (
                    cs.sole.get(&(x, f)).copied().unwrap_or(0),
                    cs.among.get(&(x, f)).copied().unwrap_or(0),
                    cs.uses.get(&(x, f)).copied().unwrap_or(0),
                    f,
                )
            })
            .collect();
        rows.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(b.2.cmp(&a.2))
                .then(a.3.cmp(&b.3))
        });
        for (s, am, u, f) in rows {
            let _ = writeln!(out, "{x}\t{f}\t{s}\t{am}\t{u}");
        }
    }
    out.push_str("\n## details of the top-ranked indexes, of 555, 1, 629, 14 and of the indexes evaluated since the second conditions step: context, field, value\n");
    for (f, d) in &cs.detail {
        for (k, v) in d {
            let _ = writeln!(out, "{v}\tfunction {f}\t{k}");
        }
    }
    std::fs::write(&a[1], out)?;
    if let Some(p) = a.get(2) {
        let mut n = String::new();
        for (f, lines) in &cs.names {
            for l in lines {
                let _ = writeln!(n, "{l}");
            }
            let _ = f;
        }
        std::fs::write(p, n)?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("cond_blockers: {e}");
        std::process::exit(1);
    }
}
