#![forbid(unsafe_code)]
//! Census of the data the data host reads (`spec/vm.md`, Data host):
//! counts only, written to `OUT.txt` (`host_data_census MANIFEST OUT.txt`),
//! and the declared types that fail a record type check to `OUT.txt.names`
//! (names, not versioned).
//!
//! - XLKR of every winning placed record (sizes, and what each word of
//!   each 8-byte pair names), for `vm_host_linked_ref_xlkr_v1`;
//! - NAME of every winning placed REFR and ACHR, for
//!   `vm_host_base_object_name_v1`;
//! - the record header flag the store reads as initially disabled, for
//!   `vm_host_is_disabled_flag_v1`;
//! - FLTV and FNAM of every winning GLOB, for `vm_host_global_fltv_v1`;
//! - LNAM of every winning FLST, for `vm_host_formlist_lnam_v1`;
//! - every VMAD object value (alias -1) naming a winning record against the
//!   declared type of its property, for `vm_record_signature_type_v1`.
use fo4_core::{Budget, Manifest};
use fo4_pex::{index::lower, index::Index, Pex};
use fo4_plugin::{
    typed::Store,
    vmad::{Property, PropertyValue, Script, Vmad},
    FormKey,
};
use fo4_vfs::Vfs;
use fo4_vm::data::SIGNATURE_TYPES;
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn check(m: &mut BTreeMap<String, [u64; 2]>, k: &str, ok: bool) {
    m.entry(k.to_string()).or_default()[usize::from(!ok)] += 1;
}
fn sig_text(s: Option<[u8; 4]>) -> String {
    s.map(|s| String::from_utf8_lossy(&s).into_owned())
        .unwrap_or_else(|| "no winning record".into())
}

fn scripts(m: &Manifest) -> Result<Index, E> {
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

struct Types<'a> {
    ix: &'a Index,
    store: &'a Store,
    plugin: usize,
    file: String,
    checks: BTreeMap<String, [u64; 2]>,
    c: Counts,
    /// Declared types failing a check (names: not versioned).
    names: Counts,
}
impl Types<'_> {
    /// Whether `a` is `b` or has it as a parent.
    fn is_a(&self, a: &str, b: &str) -> bool {
        self.ix.chain(a).iter().any(|(k, _)| *k == b)
    }
    fn value(&mut self, ty: &str, v: &PropertyValue<'_>) {
        match v {
            PropertyValue::Object(o) if o.alias < 0 && o.form_id != 0 => {
                let k = self
                    .store
                    .order()
                    .remap(self.plugin, o.form_id, &self.file, 0)
                    .ok()
                    .flatten();
                let Some(sig) = k.and_then(|k| self.store.signature(&k)) else {
                    return;
                };
                let s = sig_text(Some(sig));
                match SIGNATURE_TYPES.iter().find(|(x, _)| *x == sig) {
                    None => bump(
                        &mut self.c,
                        format!("object value of a record type not listed: {s}"),
                    ),
                    Some((_, t)) => {
                        let ok = self.is_a(ty, t) || self.is_a(t, ty);
                        if !ok {
                            bump(&mut self.names, format!("{s} {t} declared {ty}"));
                        }
                        check(
                            &mut self.checks,
                            &format!("signature type {s} {t}: declared type is it, a parent or a script extending it"),
                            ok,
                        );
                    }
                }
            }
            PropertyValue::Array(_, items) => {
                let e = ty.strip_suffix("[]").unwrap_or(ty).to_string();
                for x in items {
                    self.value(&e, x);
                }
            }
            _ => {}
        }
    }
    fn script(&mut self, s: &Script<'_>) {
        let name = lower(s.name);
        for p in &s.properties {
            if let Some(ty) = self.ix.property(&name, &lower(p.name)).map(str::to_string) {
                self.value(&ty, &p.value);
            }
        }
    }
    fn props(&mut self, script: &str, props: &[Property<'_>]) {
        for p in props {
            if let Some(ty) = self.ix.property(script, &lower(p.name)).map(str::to_string) {
                self.value(&ty, &p.value);
            }
        }
    }
}

fn run() -> Result<(), E> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("host_data_census MANIFEST OUT.txt".into());
    }
    let started = std::time::Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let mut c = Counts::new();
    let mut checks: BTreeMap<String, [u64; 2]> = BTreeMap::new();
    let order = store.order();

    // Placed records: XLKR, NAME, initially disabled.
    let placed: Vec<(FormKey, [u8; 4], bool)> = store
        .placement_cells()
        .filter(|(o, _)| !o.deleted)
        .map(|(o, _)| {
            (
                store.overrides().form_key(o.key, order),
                o.signature,
                o.disabled,
            )
        })
        .collect();
    for (k, sig, disabled) in &placed {
        let s = sig_text(Some(*sig));
        bump(&mut c, format!("placed {s}"));
        if *disabled {
            bump(
                &mut c,
                format!("placed {s} initially disabled (header flag 0x800)"),
            );
        }
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        check(
            &mut checks,
            "placed record: header flag 0x800 as read == as indexed",
            (r.flags & 0x800 != 0) == *disabled,
        );
        if matches!(sig, b"REFR" | b"ACHR") {
            let names: Vec<_> = r
                .subrecords
                .iter()
                .filter(|x| x.signature == *b"NAME")
                .collect();
            check(
                &mut checks,
                "placed REFR/ACHR: exactly one NAME",
                names.len() == 1,
            );
            if let [n] = names.as_slice() {
                let d = r.data(n);
                check(
                    &mut checks,
                    "placed REFR/ACHR: NAME is 4 bytes",
                    d.len() == 4,
                );
                if let Ok(d) = <[u8; 4]>::try_from(d) {
                    let t = order
                        .remap(plugin, u32::from_le_bytes(d), &r.source.file, n.offset)
                        .ok()
                        .flatten()
                        .and_then(|k| store.signature(&k));
                    check(
                        &mut checks,
                        "placed REFR/ACHR: NAME names a winning record",
                        t.is_some(),
                    );
                }
            }
        }
        let lnkr: Vec<_> = r
            .subrecords
            .iter()
            .filter(|x| x.signature == *b"XLKR")
            .collect();
        if lnkr.is_empty() {
            continue;
        }
        bump(&mut c, format!("placed {s} with XLKR"));
        bump(
            &mut c,
            format!("XLKR subrecords per record {}", lnkr.len().min(5)),
        );
        let mut keywords = Vec::new();
        for x in lnkr {
            let d = r.data(x);
            bump(&mut c, format!("XLKR size {}", d.len()));
            check(
                &mut checks,
                "XLKR size is a multiple of 8",
                d.len().is_multiple_of(8),
            );
            for p in d.as_chunks::<8>().0 {
                let w = |i: usize| u32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
                for (i, what) in [(0, "first"), (4, "second")] {
                    let id = w(i);
                    let named = if id == 0 {
                        "0".to_string()
                    } else {
                        match order.remap(plugin, id, &r.source.file, x.offset) {
                            Ok(Some(k)) => sig_text(store.signature(&k)),
                            _ => "unresolved".into(),
                        }
                    };
                    bump(&mut c, format!("XLKR {what} word names {named}"));
                }
                keywords.push(w(0));
            }
        }
        keywords.sort_unstable();
        check(
            &mut checks,
            "XLKR: first words distinct within a record",
            keywords.windows(2).all(|w| w[0] != w[1]),
        );
    }

    // Globals and form lists.
    let winners: Vec<(FormKey, [u8; 4], u32)> = store
        .winners()
        .filter(|w| !w.deleted)
        .map(|w| (store.overrides().form_key(w.key, order), w.signature, 0))
        .collect();
    for (k, sig, _) in winners.iter().filter(|w| matches!(&w.1, b"GLOB" | b"FLST")) {
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        if sig == b"GLOB" {
            bump(&mut c, "GLOB records");
            if r.flags & 0x40 != 0 {
                bump(&mut c, "GLOB with header flag 0x40");
            }
            let f: Vec<_> = r
                .subrecords
                .iter()
                .filter(|x| x.signature == *b"FLTV")
                .collect();
            check(&mut checks, "GLOB: exactly one FLTV", f.len() == 1);
            for x in &f {
                let d = r.data(x);
                check(&mut checks, "GLOB: FLTV is 4 bytes", d.len() == 4);
                if let Ok(d) = <[u8; 4]>::try_from(d) {
                    let v = f32::from_le_bytes(d);
                    check(&mut checks, "GLOB: FLTV is finite", v.is_finite());
                    bump(
                        &mut c,
                        format!(
                            "GLOB FLTV {}",
                            if v.fract() == 0.0 {
                                "integral"
                            } else {
                                "with a fraction"
                            }
                        ),
                    );
                }
            }
            for x in r.subrecords.iter().filter(|x| x.signature == *b"FNAM") {
                let d = r.data(x);
                let t = if d.len() == 1 && d[0].is_ascii_graphic() {
                    format!("'{}'", d[0] as char)
                } else {
                    format!("{} bytes", d.len())
                };
                bump(&mut c, format!("GLOB FNAM {t}"));
            }
        } else {
            bump(&mut c, "FLST records");
            let mut entries = 0usize;
            for x in r.subrecords.iter().filter(|x| x.signature == *b"LNAM") {
                let d = r.data(x);
                check(&mut checks, "FLST: LNAM is 4 bytes", d.len() == 4);
                if let Ok(d) = <[u8; 4]>::try_from(d) {
                    entries += 1;
                    let t = order
                        .remap(plugin, u32::from_le_bytes(d), &r.source.file, x.offset)
                        .ok()
                        .flatten()
                        .and_then(|k| store.signature(&k));
                    check(
                        &mut checks,
                        "FLST: LNAM names a winning record",
                        t.is_some(),
                    );
                    if t == Some(*b"FLST") {
                        bump(&mut c, "FLST LNAM naming a FLST");
                    }
                }
            }
            bump(
                &mut c,
                format!(
                    "FLST LNAM entries {}",
                    match entries {
                        0 => "0",
                        1..=9 => "1-9",
                        _ => ">=10",
                    }
                ),
            );
        }
    }

    // Record types of VMAD object values against declared types.
    let ix = scripts(&m)?;
    let mut t = Types {
        ix: &ix,
        store: &store,
        plugin: 0,
        file: String::new(),
        checks: BTreeMap::new(),
        c: Counts::new(),
        names: Counts::new(),
    };
    for (k, _, _) in &winners {
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            continue;
        };
        t.plugin = plugin;
        t.file = r.source.file.clone();
        for x in r.subrecords.iter().filter(|x| x.signature == *b"VMAD") {
            let Ok(v) = Vmad::read(r.data(x), r.signature, &r.source.file, x.offset, &b) else {
                continue;
            };
            for s in &v.scripts {
                t.script(s);
            }
            if let Some(q) = &v.quest {
                t.props(&lower(q.script.name), &q.script.properties);
                for a in &q.aliases {
                    for s in &a.scripts {
                        t.script(s);
                    }
                }
            }
            if let Some(term) = &v.terminal {
                t.script(&term.script);
            }
        }
    }
    let mut out = String::new();
    writeln!(out, "wall seconds\t{:.1}", started.elapsed().as_secs_f64())?;
    writeln!(out, "\n# values")?;
    for (k, v) in c.iter().chain(&t.c) {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(out, "\n# checks: holds, fails")?;
    for (k, [h, f]) in checks.iter().chain(&t.checks) {
        writeln!(out, "{h}\t{f}\t{k}")?;
    }
    std::fs::write(&args[1], out)?;
    let mut names = String::new();
    for (k, v) in &t.names {
        writeln!(names, "{v}\t{k}")?;
    }
    std::fs::write(format!("{}.names", args[1]), names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
