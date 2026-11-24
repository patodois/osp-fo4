#![forbid(unsafe_code)]
//! Census of every interface movie (`.swf` entry) of the manifest's mounts
//! with `fo4_ui` (`spec/ui.md`). Measurement support.
//!
//! `ui_census MANIFEST OUT_DIR` writes `ui-census.txt` (classes and counts
//! only, no file, symbol or text names: the file committed as
//! `spec/ui-census.txt`) and `ui-movies.txt` (one line per movie with its
//! path, sizes and counts; never versioned).
use fo4_core::{Budget, Manifest};
use fo4_ui::{tag_name, ActionKind, Compression, Limits, Movie, TextKind};
use fo4_vfs::{Location, Vfs};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn bucket(n: u64) -> String {
    match n {
        0..=4 => n.to_string(),
        5..=9 => "5-9".into(),
        10..=99 => "10-99".into(),
        100..=999 => "100-999".into(),
        1000..=9999 => "1000-9999".into(),
        _ => ">=10000".into(),
    }
}
fn size_bucket(n: u64) -> String {
    match n {
        0..=1023 => "<1 KiB".into(),
        1024..=16383 => "1-16 KiB".into(),
        16384..=131071 => "16-128 KiB".into(),
        131072..=1048575 => "128 KiB-1 MiB".into(),
        1048576..=8388607 => "1-8 MiB".into(),
        _ => ">=8 MiB".into(),
    }
}
fn table(out: &mut String, title: &str, m: &Counts) {
    let _ = writeln!(out, "\n## {title}");
    for (k, v) in m {
        let _ = writeln!(out, "{k}\t{v}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [manifest, out] = a.as_slice() else {
        return Err("usage: ui_census MANIFEST OUT_DIR".into());
    };
    let m = Manifest::load(Path::new(manifest))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let l = Limits::default();
    std::fs::create_dir_all(out)?;

    let mut lines = String::new();
    let mut c: BTreeMap<&str, Counts> = BTreeMap::new();
    let mut tags: BTreeMap<u16, (u64, u64, u64)> = BTreeMap::new();
    let mut tag_errors: Counts = BTreeMap::new();
    let mut n = 0u64;
    let cands = &vfs.candidates;
    for (i, x) in cands.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        if !k.ends_with(b".swf") {
            continue;
        }
        let winner = cands.get(i + 1).and_then(|n| n.key.as_ref()) != Some(k);
        let group = match &x.location {
            Location::Archive { archive, .. } => {
                let p = vfs.archives[*archive].path.to_ascii_lowercase();
                if p.contains("interface") {
                    "interface archive"
                } else if p.starts_with("fallout4 - ") {
                    "other base archive"
                } else if p.starts_with("dlc") {
                    "add-on archive"
                } else {
                    "other archive"
                }
            }
            Location::Loose { .. } => "loose",
        };
        let g = c.entry("movies").or_default();
        bump(
            g,
            format!("{group}, {}", if winner { "winner" } else { "overridden" }),
        );
        let b = Budget::new(512 << 20);
        let bytes = vfs.read_candidate(x, &b)?;
        let role = n;
        n += 1;
        let mv = match Movie::read(&bytes, &x.original, &l, &b) {
            Ok(mv) => mv,
            Err(e) => {
                bump(
                    c.entry("outcome").or_default(),
                    format!("refused: {}", e.code),
                );
                let _ = writeln!(lines, "{role}\t{}\t{group}\trefused\t{e}", x.original);
                continue;
            }
        };
        if !winner {
            // Census of the winners only; the overridden copy gets its line.
            let _ = writeln!(
                lines,
                "{role}\t{}\t{group}\toverridden\t{}",
                x.original, mv.stored_len
            );
            continue;
        }
        let o = c.entry("outcome").or_default();
        bump(
            o,
            if mv.tag_errors.is_empty() {
                "read, every tag parsed"
            } else {
                "read, some tags refused"
            },
        );
        bump(o, format!("end tag found: {}", mv.end_found));
        bump(
            o,
            format!("bytes after the end tag: {}", bucket(mv.trailing)),
        );
        let h = c.entry("header").or_default();
        bump(
            h,
            format!("signature {}", String::from_utf8_lossy(&mv.signature)),
        );
        bump(h, format!("compression {:?}", mv.compression));
        bump(h, format!("version {}", mv.version));
        bump(
            h,
            format!(
                "declared length equals the body: {}",
                u64::from(mv.declared_len) == mv.body_len + 8
            ),
        );
        bump(h, format!("stored size {}", size_bucket(mv.stored_len)));
        bump(h, format!("body size {}", size_bucket(mv.body_len)));
        bump(
            h,
            format!(
                "stage {} x {} twips, origin {}, {}",
                mv.stage[1] - mv.stage[0],
                mv.stage[3] - mv.stage[2],
                mv.stage[0],
                mv.stage[2]
            ),
        );
        bump(
            h,
            format!(
                "frame rate {} (8.8 raw {})",
                f32::from(mv.frame_rate_raw) / 256.0,
                mv.frame_rate_raw
            ),
        );
        bump(h, format!("root frames {}", bucket(u64::from(mv.frames))));
        bump(
            h,
            format!(
                "file attributes {}",
                mv.attributes
                    .map_or("none".into(), |a| format!("0x{a:02x}"))
            ),
        );
        bump(h, format!("background stored: {}", mv.background.is_some()));
        if mv.compression == Compression::None {
            bump(h, "uncompressed".to_string());
        }
        for (code, s) in &mv.tags {
            let t = tags.entry(*code).or_default();
            t.0 += s.count;
            t.1 += s.bytes;
            t.2 += 1;
        }
        for e in &mv.tag_errors {
            bump(
                &mut tag_errors,
                format!(
                    "{} ({}): {}",
                    tag_name(e.code),
                    e.code,
                    e.message.split(':').next().unwrap_or("")
                ),
            );
        }
        let s = c.entry("symbols").or_default();
        bump(
            s,
            format!("exports per movie {}", bucket(mv.exports.len() as u64)),
        );
        bump(
            s,
            format!("class links per movie {}", bucket(mv.classes.len() as u64)),
        );
        bump(
            s,
            format!("root class linked: {}", mv.root_class().is_some()),
        );
        bump(
            s,
            format!("imports per movie {}", bucket(mv.imports.len() as u64)),
        );
        let import_sources: BTreeSet<&str> = mv.import_sources.iter().map(|x| x.as_str()).collect();
        bump(
            s,
            format!(
                "import sources per movie {}",
                bucket(import_sources.len() as u64)
            ),
        );
        add(s, "exports (total)", mv.exports.len() as u64);
        add(s, "class links (total)", mv.classes.len() as u64);
        add(s, "imports (total)", mv.imports.len() as u64);
        add(s, "import tags (total)", mv.import_sources.len() as u64);
        let sh = c.entry("shapes").or_default();
        add(sh, "shapes (total)", mv.shapes.len() as u64);
        add(sh, "morph shapes (total)", u64::from(mv.morph_shapes));
        for x in &mv.shapes {
            bump(sh, format!("shape version {}", x.version));
            for (k, nm) in ["solid", "linear", "radial", "focal", "bitmap"]
                .iter()
                .enumerate()
            {
                add(sh, format!("fill styles {nm}"), u64::from(x.fills[k]));
            }
            add(sh, "line styles", u64::from(x.lines));
            add(sh, "straight edges", u64::from(x.straight));
            add(sh, "curved edges", u64::from(x.curved));
            add(sh, "style changes", u64::from(x.style_changes));
        }
        let t = c.entry("texts and fonts").or_default();
        for x in &mv.texts {
            match x.kind {
                TextKind::Field => {
                    bump(t, "text fields");
                    if x.chars > 0 {
                        bump(t, "text fields with initial text");
                    }
                    if x.has_variable {
                        bump(t, "text fields with a variable name");
                    }
                    if x.flags & 0x0200 != 0 {
                        bump(t, "text fields marked HTML (flag 0x0200)");
                    }
                    if x.font.is_none() {
                        bump(t, "text fields without a font id");
                    }
                }
                TextKind::Static => bump(t, "static texts"),
            }
        }
        for f in &mv.fonts {
            bump(t, format!("font version {}", f.version));
            add(
                t,
                format!("glyphs in fonts of version {}", f.version),
                u64::from(f.glyphs),
            );
            if f.version != 1 && f.glyphs == 0 {
                bump(
                    t,
                    format!("font version {} with no glyphs (name only)", f.version),
                );
            }
        }
        let sp = c.entry("sprites").or_default();
        add(sp, "sprites (total)", mv.sprites.len() as u64);
        for x in &mv.sprites {
            bump(sp, format!("sprite frames {}", bucket(u64::from(x.frames))));
            bump(sp, format!("sprite tags {}", bucket(u64::from(x.tags))));
        }
        bump(
            sp,
            format!("sprites per movie {}", bucket(mv.sprites.len() as u64)),
        );
        add(sp, "placements (total)", mv.places);
        add(
            sp,
            "named placements (total)",
            mv.instance_names.len() as u64,
        );
        add(sp, "frame labels (total)", mv.labels.len() as u64);
        let ac = c.entry("action blocks").or_default();
        let mut kinds: BTreeSet<ActionKind> = BTreeSet::new();
        for x in &mv.actions {
            kinds.insert(x.kind);
            bump(ac, format!("{:?} blocks", x.kind));
            add(ac, format!("{:?} bytes", x.kind), x.len);
            if x.kind == ActionKind::Abc {
                bump(ac, format!("Abc blocks lazy {}", x.lazy));
                bump(
                    ac,
                    format!("Abc blocks on the root timeline {}", x.sprite.is_none()),
                );
            }
        }
        bump(ac, format!("movie kinds {kinds:?}"));
        let _ = writeln!(
            lines,
            "{role}\t{}\t{group}\t{}\tv{}\tstored {}\tbody {}\tstage {:?}\tfps {}\tframes {}\ttags {}\terrors {}\texports {}\tclasses {}\troot {}\timports {}\tshapes {}\ttexts {}\tfonts {}\tsprites {}\tactions {}",
            x.original,
            String::from_utf8_lossy(&mv.signature),
            mv.version,
            mv.stored_len,
            mv.body_len,
            mv.stage,
            f32::from(mv.frame_rate_raw) / 256.0,
            mv.frames,
            mv.tags.values().map(|t| t.count).sum::<u64>(),
            mv.tag_errors.len(),
            mv.exports.len(),
            mv.classes.len(),
            mv.root_class().unwrap_or("-"),
            mv.imports.len(),
            mv.shapes.len(),
            mv.texts.len(),
            mv.fonts.len(),
            mv.sprites.len(),
            mv.actions.iter().map(|a| format!("{:?}:{}", a.kind, a.len)).collect::<Vec<_>>().join(","),
        );
    }
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# Interface movie census (ui_census; counts only, no names)"
    );
    let _ = writeln!(s, "movies\t{n}");
    for (k, m) in &c {
        table(&mut s, k, m);
    }
    let _ = writeln!(s, "\n## tags (code, name, count, body bytes, movies)");
    for (code, (cnt, bytes, movies)) in &tags {
        let _ = writeln!(s, "{code}\t{}\t{cnt}\t{bytes}\t{movies}", tag_name(*code));
    }
    table(&mut s, "refused tags", &tag_errors);
    std::fs::write(Path::new(out).join("ui-census.txt"), s)?;
    std::fs::write(Path::new(out).join("ui-movies.txt"), lines)?;
    Ok(())
}
