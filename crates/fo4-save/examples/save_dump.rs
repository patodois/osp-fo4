#![forbid(unsafe_code)]
//! One save file as text, for a person reading it: `save_dump SAVE.fos
//! [MANIFEST] [--all]` prints the parts with their offsets, the header
//! fields, the plugin lists, the location table, the global data entries,
//! a summary of the change forms by type and, with `--all`, every change
//! form (with the record type it names when a manifest is given).
//!
//! The output carries the save's personal content: print it to a terminal
//! or a file outside the repository; it is never committed. The save is
//! only read; the screenshot is not written anywhere.
use fo4_core::{Budget, FileSource, Limits, Manifest};
use fo4_plugin::typed::Store;
use fo4_save::Save;
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

mod support;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = args.iter().any(|a| a == "--all");
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if pos.is_empty() || pos.len() > 2 {
        return Err("save_dump SAVE.fos [MANIFEST] [--all]".into());
    }
    let src = FileSource::open(Path::new(pos[0]))?;
    let manifest = pos
        .get(1)
        .map(|p| Manifest::load(Path::new(p)))
        .transpose()?;
    let limits = manifest
        .as_ref()
        .map(|m| m.limits.clone())
        .unwrap_or_default();
    let s = Save::read(&src, &limits, &Budget::new(limits.max_memory))?;
    let store = manifest
        .as_ref()
        .map(|m| Store::open(m, &Budget::new(m.limits.max_override_memory)))
        .transpose()?;
    let resolver = match (&store, &manifest) {
        (Some(st), Some(m)) => Some(support::Resolver::new(st, &s, &m.root)),
        _ => None,
    };
    let mut o = String::new();
    dump(&mut o, &s, resolver.as_ref(), all, &src, &limits)?;
    print!("{o}");
    Ok(())
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn dump(
    o: &mut String,
    s: &Save,
    r: Option<&support::Resolver<'_>>,
    all: bool,
    src: &FileSource,
    l: &Limits,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(o, "length {}; framed {}", s.length, s.parts.total())?;
    writeln!(o, "parts {:#?}", s.parts)?;
    let h = &s.header;
    writeln!(
        o,
        "\nheader ({} bytes), labels are candidate meanings",
        s.header_size
    )?;
    writeln!(o, "  version {}  save number {}", h.version, h.save_number)?;
    writeln!(
        o,
        "  character {:?}  level {}",
        lossy(&h.character),
        h.level
    )?;
    writeln!(o, "  location {:?}", lossy(&h.location))?;
    writeln!(
        o,
        "  play time {:?}  race {:?}",
        lossy(&h.play_time),
        lossy(&h.race)
    )?;
    writeln!(
        o,
        "  sex {}  experience {} / {}  file time {}",
        h.sex, h.experience, h.next_experience, h.file_time
    )?;
    writeln!(
        o,
        "  screenshot {} x {} at {} ({} bytes)",
        h.shot_width, h.shot_height, s.screenshot.offset, s.screenshot.length
    )?;
    writeln!(
        o,
        "\nform version {}  game version {:?}",
        s.form_version,
        lossy(&s.game_version)
    )?;
    writeln!(o, "plugins ({}):", s.plugins.len())?;
    for (i, p) in s.plugins.iter().enumerate() {
        writeln!(o, "  {i:02X} {}", lossy(p))?;
    }
    writeln!(o, "second list ({}):", s.light_plugins.len())?;
    for (i, p) in s.light_plugins.iter().enumerate() {
        writeln!(o, "  FE{i:03X} {}", lossy(p))?;
    }
    writeln!(o, "\nlocation table {:#?}", s.locations)?;
    for (t, table) in s.globals.iter().enumerate() {
        writeln!(o, "\nglobal data table {}:", t + 1)?;
        for g in table {
            writeln!(
                o,
                "  type {:>4} at {:>8} length {}",
                g.kind, g.data.offset, g.data.length
            )?;
        }
    }
    let mut by_kind: BTreeMap<u8, (u64, u64, u64)> = BTreeMap::new();
    for f in &s.change_forms {
        let e = by_kind.entry(f.kind()).or_default();
        e.0 += 1;
        e.1 += u64::from(f.compressed());
        e.2 += u64::from(f.length1);
    }
    writeln!(
        o,
        "\nchange forms ({}): type, entries, compressed, stored bytes",
        s.change_forms.len()
    )?;
    for (k, (n, z, b)) in &by_kind {
        writeln!(o, "  {k:>2} {n:>6} {z:>6} {b:>9}")?;
    }
    if all {
        writeln!(
            o,
            "\nevery change form: offset ref kind form-id flags type version len1 len2 [record]"
        )?;
        for f in &s.change_forms {
            let id = s.resolve(f.form);
            let target = match (r, id) {
                (Some(r), Some(id)) => support::class(&r.target(id)),
                _ => String::new(),
            };
            writeln!(
                o,
                "  {:>8} {:02X}{:02X}{:02X} {:?} {} {:08X} {:>2} {} {} {} {}",
                f.offset,
                f.form.0[0],
                f.form.0[1],
                f.form.0[2],
                f.form.kind(),
                id.map(|x| format!("{x:08X}")).unwrap_or("-".into()),
                f.flags,
                f.kind(),
                f.version,
                f.length1,
                f.length2,
                target
            )?;
        }
        // The first payload of each type, to see its shape.
        let mut shown = std::collections::BTreeSet::new();
        for f in &s.change_forms {
            if !shown.insert(f.kind()) {
                continue;
            }
            let p = f.payload(src, l, &Budget::new(l.max_memory))?;
            let head: String = p.iter().take(48).map(|b| format!("{b:02x}")).collect();
            writeln!(
                o,
                "  first type {:>2} payload ({} bytes): {head}",
                f.kind(),
                p.len()
            )?;
        }
    }
    writeln!(o, "\nform id array: {} entries", s.form_ids.len())?;
    writeln!(o, "second array: {:08X?}", s.second_array)?;
    writeln!(
        o,
        "closing block at {}: {:02x?}",
        s.closing.offset, s.closing_bytes
    )?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
