#![forbid(unsafe_code)]
//! Census of every VMAD subrecord of the winning records with
//! `fo4_plugin::vmad` (`spec/vmad.md`): outcomes and tails by record type,
//! value kinds and flags, the TERM fragment block, and cross-checks against
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
    };
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
                }
            }
        }
    }
    let mut out = String::new();
    writeln!(out, "records with VMAD (winners, not deleted)\t{records}")?;
    writeln!(out, "VMAD bytes\t{bytes}")?;
    writeln!(
        out,
        "VMAD bytes read (scripts and TERM blocks)\t{read_bytes}"
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
