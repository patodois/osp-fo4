#![forbid(unsafe_code)]
//! Census of every VMAD subrecord of the winning records with
//! `fo4_plugin::vmad` (`spec/vmad.md`): outcomes and tails by record type,
//! value kinds and flags, the TERM fragment block, the QUST block (stage
//! fragments and aliases), and cross-checks against
//! the scripts of the archives read with `fo4_pex` (each script name against
//! a `.pex` entry, each property against that script's declared properties
//! and type, each object FormID against the winning records). Measurement
//! support; `vmad_census MANIFEST OUT.txt` writes counts only.
use fo4_core::{Budget, Manifest};
use fo4_pex::{index::lower, index::Index, Pex};
use fo4_plugin::{
    typed::Store,
    vmad::{Property, PropertyValue, Script, Vmad},
};
use fo4_vfs::Vfs;
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn check(m: &mut BTreeMap<String, [u64; 2]>, k: &str, ok: bool) {
    m.entry(k.to_string()).or_default()[usize::from(!ok)] += 1;
}

/// Every `.pex` entry of the archives in one index.
fn scripts(m: &Manifest) -> Result<Index, Box<dyn std::error::Error>> {
    let vfs = Vfs::mount(m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut ix = Index::default();
    for c in &vfs.candidates {
        let Some(k) = &c.key else { continue };
        if !k.ends_with(b".pex") {
            continue;
        }
        let b = Budget::new(m.limits.max_memory);
        let bytes = vfs.read_candidate(c, &b)?;
        let name = String::from_utf8_lossy(k).into_owned();
        ix.add(&Pex::read(&bytes, &name, &b)?);
    }
    Ok(ix)
}

/// Whether a stored kind fits a declared `.pex` type.
fn kind_fits(kind: u8, ty: &str, ix: &Index) -> bool {
    let simple = |k: u8, t: &str| match k {
        1 => {
            ix.scripts.contains_key(t) && !matches!(t, "int" | "float" | "bool" | "string" | "var")
        }
        2 => t == "string",
        3 => t == "int",
        4 => t == "float",
        5 => t == "bool",
        7 => t.contains('#'),
        _ => false,
    };
    match kind {
        11..=15 => ty.strip_suffix("[]").is_some_and(|t| simple(kind - 10, t)),
        17 => ty.strip_suffix("[]").is_some_and(|t| t.contains('#')),
        k => simple(k, ty),
    }
}

struct Ctx<'a> {
    ix: &'a Index,
    store: &'a Store,
    plugin: usize,
    file: String,
    c: Counts,
    checks: BTreeMap<String, [u64; 2]>,
    /// Per winning QUST: the alias indices of its VMAD block and the u32
    /// values of its ALST subrecords.
    quests: std::collections::HashMap<fo4_plugin::FormKey, (Vec<i16>, Vec<u32>)>,
}

/// Engine types an object property's declared type is classified by: the
/// first of these in the declared type's chain.
const TYPE_CLASSES: [&str; 8] = [
    "referencealias",
    "refcollectionalias",
    "locationalias",
    "actor",
    "objectreference",
    "quest",
    "alias",
    "form",
];
fn type_class(ix: &Index, ty: &str) -> String {
    let t = ty.strip_suffix("[]").unwrap_or(ty);
    for (k, _) in ix.chain(t) {
        if TYPE_CLASSES.contains(&k) {
            return k.to_string();
        }
    }
    "other".to_string()
}
impl Ctx<'_> {
    fn object(&mut self, sig: &str, o: &fo4_plugin::vmad::ObjectRef) {
        bump(&mut self.c, format!("object value zero word {}", o.zero));
        bump(
            &mut self.c,
            format!(
                "object value alias {} on {sig}",
                if o.alias < 0 {
                    "-1".to_string()
                } else {
                    ">=0".to_string()
                }
            ),
        );
        if o.alias >= 0 {
            let key = self
                .store
                .order()
                .remap(self.plugin, o.form_id, &self.file, 0)
                .ok()
                .flatten();
            let quest = key.as_ref().and_then(|k| self.quests.get(k));
            check(
                &mut self.checks,
                "object value with alias >= 0 names a winning QUST",
                quest.is_some(),
            );
            if let Some((block, alst)) = quest {
                let (in_block, in_alst) = (
                    block.contains(&o.alias),
                    alst.contains(&(o.alias as i32 as u32)),
                );
                check(
                    &mut self.checks,
                    "object value with alias >= 0: the quest's ALST values hold the index",
                    in_alst,
                );
                bump(
                    &mut self.c,
                    format!("object value with alias >= 0: the quest's VMAD block has an entry of that index {in_block}"),
                );
            }
            return;
        }
        if o.form_id == 0 {
            bump(&mut self.c, "object value FormID 0 (alias -1)");
            return;
        }
        let key = self
            .store
            .order()
            .remap(self.plugin, o.form_id, &self.file, 0)
            .ok()
            .flatten();
        let found = key.is_some_and(|k| self.store.signature(&k).is_some());
        check(
            &mut self.checks,
            "object value FormID (alias -1, not 0) names a winning record",
            found,
        );
    }
    fn value(&mut self, sig: &str, v: &PropertyValue<'_>) {
        match v {
            PropertyValue::Object(o) => self.object(sig, o),
            PropertyValue::Struct(m) => {
                for p in m {
                    self.member(sig, p);
                }
            }
            PropertyValue::Array(_, v) => {
                for x in v {
                    self.value(sig, x);
                }
            }
            PropertyValue::StructArray(v) => {
                for m in v {
                    for p in m {
                        self.member(sig, p);
                    }
                }
            }
            _ => {}
        }
    }
    fn member(&mut self, sig: &str, p: &Property<'_>) {
        bump(
            &mut self.c,
            format!("struct member kind {} flags {}", p.kind, p.flags),
        );
        self.value(sig, &p.value);
    }
    fn script(&mut self, sig: &str, s: &Script<'_>, place: &str) {
        bump(&mut self.c, format!("{place} script flags {}", s.flags));
        let name = lower(s.name);
        let known = self.ix.scripts.contains_key(&name);
        check(
            &mut self.checks,
            &format!("{place} script name is a .pex object of the archives"),
            known,
        );
        for p in &s.properties {
            bump(
                &mut self.c,
                format!("property kind {} flags {}", p.kind, p.flags),
            );
            if known {
                let ty = self.ix.property(&name, &lower(p.name)).map(str::to_string);
                check(
                    &mut self.checks,
                    &format!("{place} property names a property of the script or a parent"),
                    ty.is_some(),
                );
                if let Some(ty) = ty {
                    let aliased = match &p.value {
                        PropertyValue::Object(o) => o.alias >= 0,
                        PropertyValue::Array(_, v) => v
                            .iter()
                            .any(|x| matches!(x, PropertyValue::Object(o) if o.alias >= 0)),
                        _ => false,
                    };
                    if aliased {
                        bump(
                            &mut self.c,
                            format!(
                                "object value with alias >= 0: declared type class {}",
                                type_class(self.ix, &ty)
                            ),
                        );
                    }
                    let fits = matches!(p.kind, 0) || kind_fits(p.kind, &ty, self.ix);
                    check(
                        &mut self.checks,
                        &format!("{place} property kind fits the declared type (kind 0 excepted)"),
                        fits,
                    );
                    if !fits {
                        bump(
                            &mut self.c,
                            format!(
                                "property kind {} against declared {}",
                                p.kind,
                                if ty.ends_with("[]") {
                                    "array"
                                } else if ty.contains('#') {
                                    "struct"
                                } else if matches!(
                                    ty.as_str(),
                                    "int" | "float" | "bool" | "string" | "var"
                                ) {
                                    ty.as_str()
                                } else {
                                    "object"
                                }
                            ),
                        );
                    }
                }
            }
            self.value(sig, &p.value);
        }
    }
}

/// Counts and checks of one QUST block.
fn quest_block(ctx: &mut Ctx<'_>, r: &fo4_plugin::Record, q: &fo4_plugin::vmad::QuestBlock<'_>) {
    bump(&mut ctx.c, format!("QUST block version {}", q.version));
    bump(
        &mut ctx.c,
        format!(
            "QUST block script name empty {}, fragments {}",
            q.script.name.is_empty(),
            if q.fragments.is_empty() { "0" } else { ">0" }
        ),
    );
    if !q.script.name.is_empty() {
        ctx.script("QUST", &q.script, "QUST block");
    }
    let block = lower(q.script.name);
    let mut stages: Vec<u16> = Vec::new();
    for f in &q.fragments {
        bump(
            &mut ctx.c,
            format!("QUST fragment word {} byte {}", f.word, f.byte),
        );
        check(
            &mut ctx.checks,
            "QUST fragment script == block script",
            f.script == q.script.name,
        );
        let declared = ctx.ix.function(&block, &lower(f.function));
        check(
            &mut ctx.checks,
            "QUST fragment function declared by the block script",
            declared.is_some(),
        );
        if let Some(c) = declared {
            bump(
                &mut ctx.c,
                format!(
                    "QUST fragment function params {} returns {}",
                    c.decl.params, c.decl.return_type
                ),
            );
        }
        let has_stage = r.subrecords.iter().any(|s| {
            s.signature == *b"INDX" && r.data(s).get(..2) == Some(&f.stage.to_le_bytes()[..])
        });
        check(
            &mut ctx.checks,
            "QUST fragment stage is the first u16 of an INDX subrecord of the quest",
            has_stage,
        );
        stages.push(f.stage);
    }
    let mut aliases: Vec<i16> = Vec::new();
    for a in &q.aliases {
        bump(
            &mut ctx.c,
            format!(
                "QUST alias entry zero word {} version {} object format {}",
                a.object.zero, a.version, a.object_format
            ),
        );
        check(
            &mut ctx.checks,
            "QUST alias entry FormID is the quest's own",
            a.object.form_id == r.form_id,
        );
        check(
            &mut ctx.checks,
            "QUST alias entry index >= 0",
            a.object.alias >= 0,
        );
        let alst = r.subrecords.iter().any(|s| {
            s.signature == *b"ALST" && r.data(s) == (a.object.alias as i32 as u32).to_le_bytes()
        });
        check(
            &mut ctx.checks,
            "QUST alias entry index is the value of an ALST subrecord of the quest",
            alst,
        );
        bump(
            &mut ctx.c,
            format!("QUST alias entry scripts {}", a.scripts.len().min(5)),
        );
        for sc in &a.scripts {
            ctx.script("QUST", sc, "QUST alias");
        }
        aliases.push(a.object.alias);
    }
    aliases.sort_unstable();
    check(
        &mut ctx.checks,
        "QUST alias entry indices distinct within a block",
        aliases.windows(2).all(|w| w[0] != w[1]),
    );
    stages.sort_unstable();
    bump(
        &mut ctx.c,
        format!(
            "QUST block with two fragments of one stage {}",
            stages.windows(2).any(|w| w[0] == w[1])
        ),
    );
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("vmad_census MANIFEST OUT.txt".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let ix = scripts(&m)?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let winners: Vec<_> = store
        .winners()
        .filter(|w| !w.deleted)
        .map(|w| (store.overrides().form_key(w.key, store.order()), w.plugin))
        .collect();
    let mut ctx = Ctx {
        ix: &ix,
        store: &store,
        plugin: 0,
        file: String::new(),
        c: Counts::new(),
        checks: BTreeMap::new(),
        quests: Default::default(),
    };
    // First pass: the alias indices of every QUST block and the ALST
    // values of the quest.
    for (key, _) in winners
        .iter()
        .filter(|(k, _)| store.signature(k) == Some(*b"QUST"))
    {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(key, &b)? else {
            continue;
        };
        let alst = r
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"ALST")
            .filter_map(|s| r.data(s).try_into().ok().map(u32::from_le_bytes))
            .collect();
        let mut block = Vec::new();
        for s in r.subrecords.iter().filter(|s| s.signature == *b"VMAD") {
            if let Ok(v) = Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, &b) {
                if let Some(q) = &v.quest {
                    block.extend(q.aliases.iter().map(|a| a.object.alias));
                }
            }
        }
        ctx.quests.insert(key.clone(), (block, alst));
    }
    let mut outcomes = Counts::new();
    let mut tails = Counts::new();
    let mut bytes = 0u64;
    let mut read_bytes = 0u64;
    let mut records = 0u64;
    for (key, plugin) in &winners {
        let b = Budget::new(m.limits.max_memory);
        let Some(r) = store.raw(key, &b)? else {
            continue;
        };
        let sig = String::from_utf8_lossy(&r.signature).into_owned();
        let subs: Vec<_> = r
            .subrecords
            .iter()
            .filter(|s| s.signature == *b"VMAD")
            .collect();
        if subs.is_empty() {
            continue;
        }
        records += 1;
        bump(
            &mut ctx.c,
            format!("VMAD subrecords per record {}", subs.len()),
        );
        ctx.plugin = *plugin;
        ctx.file = r.source.file.clone();
        for s in subs {
            let d = r.data(s);
            bytes += d.len() as u64;
            match Vmad::read(d, r.signature, &r.source.file, s.offset, &b) {
                Err(e) => bump(
                    &mut outcomes,
                    format!("{sig} error {} (expected {})", e.code, e.expected),
                ),
                Ok(v) => {
                    bump(&mut outcomes, format!("{sig} read"));
                    read_bytes += (d.len() - v.tail.len()) as u64;
                    bump(&mut ctx.c, format!("version {}", v.version));
                    bump(
                        &mut ctx.c,
                        format!("scripts per VMAD {}", v.scripts.len().min(5)),
                    );
                    if v.tail.is_empty() {
                        bump(&mut tails, format!("{sig} no bytes after what is read"));
                    } else {
                        bump(
                            &mut tails,
                            format!("{sig} bytes after what is read: records"),
                        );
                        *tails
                            .entry(format!("{sig} bytes after what is read: bytes"))
                            .or_default() += v.tail.len() as u64;
                    }
                    for sc in &v.scripts {
                        ctx.script(&sig, sc, "list");
                    }
                    if let Some(t) = &v.terminal {
                        bump(&mut ctx.c, format!("TERM block version {}", t.version));
                        bump(
                            &mut ctx.c,
                            format!("TERM block fragments {}", t.fragments.len().min(10)),
                        );
                        ctx.script(&sig, &t.script, "TERM block");
                        let mut idx: Vec<u32> = t.fragments.iter().map(|f| f.index).collect();
                        for f in &t.fragments {
                            bump(&mut ctx.c, format!("TERM fragment byte {}", f.byte));
                            check(
                                &mut ctx.checks,
                                "TERM fragment script == block script",
                                f.script == t.script.name,
                            );
                            let fname = lower(f.function);
                            let declared = ix.function(&lower(t.script.name), &fname);
                            check(
                                &mut ctx.checks,
                                "TERM fragment function declared by the block script",
                                declared.is_some(),
                            );
                            if let Some(c) = declared {
                                bump(
                                    &mut ctx.c,
                                    format!(
                                        "TERM fragment function params {} returns {}",
                                        c.decl.params, c.decl.return_type
                                    ),
                                );
                            }
                            check(&mut ctx.checks, "TERM fragment index >= 1", f.index >= 1);
                        }
                        idx.sort_unstable();
                        let distinct = idx.windows(2).all(|w| w[0] != w[1]);
                        check(
                            &mut ctx.checks,
                            "TERM fragment indices distinct within a block",
                            distinct,
                        );
                    } else if sig == "TERM" {
                        bump(&mut ctx.c, "TERM without a block");
                    }
                    if let Some(q) = &v.quest {
                        quest_block(&mut ctx, &r, q);
                    } else if sig == "QUST" {
                        bump(&mut ctx.c, "QUST without a block");
                    }
                }
            }
        }
    }
    let mut out = String::new();
    writeln!(out, "records with VMAD (winners, not deleted)\t{records}")?;
    writeln!(out, "VMAD bytes\t{bytes}")?;
    writeln!(
        out,
        "VMAD bytes read (scripts; TERM, QUST, INFO, PACK, SCEN and PERK blocks)\t{read_bytes}"
    )?;
    writeln!(out, ".pex objects indexed\t{}", ix.scripts.len())?;
    writeln!(out, "wall seconds\t{:.1}", started.elapsed().as_secs_f64())?;
    for (t, m) in [
        ("outcomes by record type", &outcomes),
        ("bytes after what is read, by record type", &tails),
        ("values", &ctx.c),
    ] {
        writeln!(out, "\n# {t}")?;
        for (k, v) in m {
            writeln!(out, "{v}\t{k}")?;
        }
    }
    writeln!(out, "\n# checks: holds, fails")?;
    for (k, [h, f]) in &ctx.checks {
        writeln!(out, "{h}\t{f}\t{k}")?;
    }
    std::fs::write(&args[1], out)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
