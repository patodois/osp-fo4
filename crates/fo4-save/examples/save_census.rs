#![forbid(unsafe_code)]
//! Census of every save file (`*.fos`) of one folder with `fo4_save`:
//! outcomes, bytes per part, interpreted against raw bytes, header field
//! widths and checks, plugin lists against the manifest, the global data
//! tables, the change-form table by type, version, width, compression and
//! flag bits, and how the form references resolve against the plugin data
//! under the candidate reading and under alternative readings
//! (`spec/save.md`). Measurement support.
//!
//! `save_census MANIFEST SAVE_DIR OUT.txt` writes counts, sizes, field
//! widths, type codes and flag-bit histograms only: no name, text, form id,
//! play time, path or screenshot byte of any save. Its output is committed
//! as `spec/save-census.txt`. The save folder is only listed and read.
use fo4_core::{Budget, FileSource, Limits, Manifest, ReadAt};
use fo4_plugin::{
    typed::{Cell, Store},
    FormKey,
};
use fo4_save::{RefId, RefKind, Save};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
    path::Path,
    time::Instant,
};

mod support;
use support::{class, Resolver, Target};

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn check(m: &mut BTreeMap<String, [u64; 2]>, k: &str, ok: bool) {
    m.entry(k.to_string()).or_default()[usize::from(!ok)] += 1;
}
#[derive(Default, Clone, Copy)]
struct Range {
    n: u64,
    min: u64,
    max: u64,
    sum: u64,
}
impl Range {
    fn put(&mut self, v: u64) {
        if self.n == 0 || v < self.min {
            self.min = v;
        }
        self.max = self.max.max(v);
        self.n += 1;
        self.sum += v;
    }
}
fn ranges(m: &mut BTreeMap<String, Range>, k: impl Into<String>, v: u64) {
    m.entry(k.into()).or_default().put(v);
}

/// One alternative reading of the three-byte reference.
#[derive(Clone, Copy)]
struct Reading {
    name: &'static str,
    big_endian: bool,
    one_based: bool,
}
const READINGS: [Reading; 4] = [
    Reading {
        name: "big-endian word, 1-based index (candidate)",
        big_endian: true,
        one_based: true,
    },
    Reading {
        name: "big-endian word, 0-based index",
        big_endian: true,
        one_based: false,
    },
    Reading {
        name: "little-endian word, 1-based index",
        big_endian: false,
        one_based: true,
    },
    Reading {
        name: "little-endian word, 0-based index",
        big_endian: false,
        one_based: false,
    },
];
fn resolve_with(save: &Save, r: RefId, how: Reading) -> Option<u32> {
    let b = if how.big_endian {
        r.0
    } else {
        [r.0[2], r.0[1], r.0[0]]
    };
    let x = RefId(b);
    match x.kind() {
        RefKind::Indexed => {
            let i = x.value() as usize;
            let i = if how.one_based { i.checked_sub(1)? } else { i };
            save.form_ids.get(i).copied()
        }
        RefKind::Direct => Some(x.value()),
        RefKind::Created => Some(0xff00_0000 | x.value()),
        RefKind::Other => None,
    }
}

#[derive(Default)]
struct Census {
    files: u64,
    bytes: u64,
    framed: u64,
    parts: Counts,
    raw: Counts,
    outcomes: Counts,
    checks: BTreeMap<String, [u64; 2]>,
    widths: BTreeMap<String, Range>,
    values: Counts,
    plugins: Counts,
    globals: BTreeMap<String, Range>,
    cf_kind: BTreeMap<u8, u64>,
    cf_version: Counts,
    cf_width: Counts,
    cf_compressed: Counts,
    cf_lengths: BTreeMap<String, Range>,
    cf_bits: BTreeMap<u8, [u64; 32]>,
    ref_kinds: Counts,
    resolution: Counts,
    kind_signature: BTreeMap<u8, Counts>,
    readings: BTreeMap<&'static str, [u64; 3]>,
    form_ids: Counts,
    second: Counts,
    closing: Counts,
    moved: Counts,
    zlib_in: u64,
    zlib_out: u64,
}

/// Whether a text begins with decimal digits and `d.`, digits and `h.`,
/// digits and `m.`.
fn play_time_shape(t: &[u8]) -> bool {
    let mut rest = t;
    for unit in [b"d.", b"h.", b"m."] {
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 || !rest[digits..].starts_with(unit) {
            return false;
        }
        rest = &rest[digits + 2..];
    }
    true
}

/// Placed references and actors of the plugin data: the cell their winning
/// record sits in.
type Placed = HashMap<FormKey, FormKey>;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("save_census MANIFEST SAVE_DIR OUT.txt".into());
    }
    let start = Instant::now();
    let m = Manifest::load(Path::new(&args[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let placed: Placed = store
        .placement_cells()
        .map(|(o, cell)| (store.overrides().form_key(o.key, store.order()), cell))
        .collect();
    let paths = support::saves(Path::new(&args[1]))?;
    let declared_build = m
        .build
        .as_deref()
        .and_then(|b| b.split('-').nth(1))
        .unwrap_or("")
        .to_owned();
    let mut c = Census::default();
    for path in &paths {
        let src = FileSource::open(path)?;
        c.files += 1;
        c.bytes += src.len();
        let b = Budget::new(m.limits.max_memory);
        let save = match Save::read(&src, &m.limits, &b) {
            Ok(s) => s,
            Err(e) => {
                bump(&mut c.outcomes, format!("refused: {}", e.code));
                continue;
            }
        };
        let total = save.parts.total();
        c.framed += total;
        bump(
            &mut c.outcomes,
            if total == save.length {
                "read to the last byte"
            } else {
                "framed bytes differ from the length"
            },
        );
        file(
            &mut c,
            &save,
            &src,
            path,
            &m.limits,
            &store,
            &m,
            &declared_build,
        )?;
        moved(&mut c, &save, &src, &m.limits, &store, &m, &placed)?;
    }
    let mut o = String::new();
    write(&mut o, &c, start.elapsed().as_secs_f64())?;
    std::fs::write(&args[2], o)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn file(
    c: &mut Census,
    s: &Save,
    src: &FileSource,
    path: &Path,
    l: &Limits,
    store: &Store,
    m: &Manifest,
    declared_build: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let p = &s.parts;
    for (k, v) in [
        ("01 magic and header size word", p.preamble),
        ("02 header", p.header),
        ("03 screenshot", p.screenshot),
        (
            "04 form version, game version, plugin block",
            p.versions_and_plugins,
        ),
        ("05 location table", p.location_table),
        ("06 global data tables 1 and 2", p.globals_1_2),
        ("07 change-form entry headers", p.change_form_headers),
        ("08 change-form payloads", p.change_form_payloads),
        ("09 global data table 3", p.globals_3),
        ("10 form id array", p.form_id_array),
        ("11 second array", p.second_array),
        ("12 closing block", p.closing),
    ] {
        add(&mut c.parts, k, v);
    }
    let global_bodies: u64 = s.globals.iter().flatten().map(|g| g.data.length).sum();
    add(&mut c.raw, "raw: screenshot", p.screenshot);
    add(&mut c.raw, "raw: global data bodies", global_bodies);
    add(
        &mut c.raw,
        "raw: change-form payloads",
        p.change_form_payloads,
    );
    add(&mut c.raw, "raw: closing block", p.closing);
    add(
        &mut c.raw,
        "interpreted: every other byte",
        p.total() - p.screenshot - global_bodies - p.change_form_payloads - p.closing,
    );

    // Header.
    let h = &s.header;
    bump(&mut c.values, format!("header version {}", h.version));
    ranges(&mut c.widths, "header size word", u64::from(s.header_size));
    for (k, t) in [
        ("text 1 (candidate character name) bytes", &h.character),
        ("text 2 (candidate location) bytes", &h.location),
        ("text 3 (candidate play time) bytes", &h.play_time),
        ("text 4 (candidate race editor id) bytes", &h.race),
    ] {
        ranges(&mut c.widths, k, t.len() as u64);
    }
    check(
        &mut c.checks,
        "header: text 3 begins <digits>d.<digits>h.<digits>m.",
        play_time_shape(&h.play_time),
    );
    check(
        &mut c.checks,
        "header: text 4 is ASCII letters and digits only",
        h.race.iter().all(|b| b.is_ascii_alphanumeric()),
    );
    check(&mut c.checks, "header: u16 field is 0 or 1", h.sex <= 1);
    check(
        &mut c.checks,
        "header: word 3 (candidate level) >= 1",
        h.level >= 1,
    );
    check(
        &mut c.checks,
        "header: both f32 finite, >= 0, the second greater",
        h.experience.is_finite()
            && h.next_experience.is_finite()
            && h.experience >= 0.0
            && h.next_experience > h.experience,
    );
    // Windows file time (100 ns since 1601) against the file's modification time.
    let mtime = std::fs::metadata(path)?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let ft_unix = (h.file_time / 10_000_000).saturating_sub(11_644_473_600);
    let d = ft_unix.abs_diff(mtime);
    bump(
        &mut c.values,
        format!(
            "u64 as a Windows file time, distance to file modification time: {}",
            match d {
                0..=10 => "<= 10 s",
                11..=120 => "11 s to 2 min",
                121..=86_400 => "2 min to 1 day",
                _ => "> 1 day",
            }
        ),
    );
    bump(
        &mut c.values,
        format!("screenshot {} x {}", h.shot_width, h.shot_height),
    );
    let shot = fo4_core::read_bytes(
        src,
        s.screenshot.offset,
        s.screenshot.length,
        l,
        &Budget::new(l.max_memory),
    )?;
    let alpha = shot
        .iter()
        .skip(3)
        .step_by(4)
        .filter(|b| **b == 255)
        .count() as u64;
    add(&mut c.values, "screenshot pixels", shot.len() as u64 / 4);
    add(
        &mut c.values,
        "screenshot pixels whose 4th byte is 255",
        alpha,
    );

    // Versions.
    bump(&mut c.values, format!("form version {}", s.form_version));
    let gv = String::from_utf8_lossy(&s.game_version).into_owned();
    check(
        &mut c.checks,
        "game version text is four dot-separated decimal numbers",
        gv.split('.').count() == 4 && gv.split('.').all(|x| x.parse::<u32>().is_ok()),
    );
    check(
        &mut c.checks,
        &format!(
            "form version 68 iff game version text starts with the manifest's declared build (form version {})",
            s.form_version
        ),
        (s.form_version == 68) == (!declared_build.is_empty() && gv.starts_with(declared_build)),
    );

    // Plugins.
    let r = Resolver::new(store, s, &m.root);
    bump(
        &mut c.plugins,
        format!("first list: {} names", s.plugins.len()),
    );
    bump(
        &mut c.plugins,
        format!("second list: {} names", s.light_plugins.len()),
    );
    let order: Vec<String> = store
        .order()
        .headers()
        .iter()
        .map(|h| h.name.to_ascii_lowercase())
        .collect();
    for (i, stored) in s.plugins.iter().enumerate() {
        let stored = String::from_utf8_lossy(stored).into_owned();
        let n = stored.to_ascii_lowercase();
        check(
            &mut c.checks,
            "plugins: first-list name at the same index of the manifest's load order",
            order.get(i) == Some(&n),
        );
        check(
            &mut c.checks,
            "plugins: first-list name is a file in the data folder",
            fo4_core::within(&m.root, &stored).is_ok_and(|x| x.is_file()),
        );
        check(
            &mut c.checks,
            "plugins: first-list name ends .esm",
            n.ends_with(".esm"),
        );
    }
    for (n, inst) in s.light_plugins.iter().zip(&r.light_installed) {
        check(
            &mut c.checks,
            "plugins: second-list name ends .esl",
            n.to_ascii_lowercase().ends_with(b".esl"),
        );
        check(
            &mut c.checks,
            "plugins: second-list name is a file in the data folder",
            *inst,
        );
    }
    check(
        &mut c.checks,
        "plugins: second list empty iff form version 68",
        s.light_plugins.is_empty() == (s.form_version == 68),
    );

    // Location table.
    check(
        &mut c.checks,
        "location table: the 15 reserved words are zero",
        s.locations.reserved.iter().all(|w| *w == 0),
    );
    for (i, n) in s.locations.global_counts.iter().enumerate() {
        bump(
            &mut c.values,
            format!("global data table {} count {}", i + 1, n),
        );
    }

    // Global data.
    for (t, table) in s.globals.iter().enumerate() {
        let kinds: Vec<u32> = table.iter().map(|g| g.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort_unstable();
        sorted.dedup();
        check(
            &mut c.checks,
            &format!("global data table {}: types ascending and distinct", t + 1),
            sorted == kinds,
        );
        for g in table {
            ranges(
                &mut c.globals,
                format!("table {} type {:>4}", t + 1, g.kind),
                g.data.length,
            );
        }
    }

    // Change forms.
    let mut seen = HashSet::new();
    for f in &s.change_forms {
        let k = f.kind();
        *c.cf_kind.entry(k).or_default() += 1;
        bump(
            &mut c.cf_version,
            format!(
                "file form version {}, entry version {}",
                s.form_version, f.version
            ),
        );
        bump(&mut c.cf_width, format!("width code {}", f.width_code()));
        let need = f.length1.max(f.length2);
        let minimal = if need <= 0xff {
            0
        } else if need <= 0xffff {
            1
        } else {
            2
        };
        check(
            &mut c.checks,
            "change forms: width code is the smallest that holds both lengths",
            f.width_code() == minimal,
        );
        check(
            &mut c.checks,
            "change forms: reference appears once per file",
            seen.insert(f.form),
        );
        for (bit, slot) in c.cf_bits.entry(k).or_insert([0; 32]).iter_mut().enumerate() {
            if f.flags >> bit & 1 == 1 {
                *slot += 1;
            }
        }
        ranges(
            &mut c.cf_lengths,
            format!("type {k:>2} stored length"),
            u64::from(f.length1),
        );
        if f.compressed() {
            bump(&mut c.cf_compressed, format!("type {k:>2} compressed"));
            let b = Budget::new(l.max_memory);
            let ok = f.payload(src, l, &b).is_ok();
            check(
                &mut c.checks,
                "change forms: second length not 0 -> exact zlib stream of that output length",
                ok,
            );
            check(
                &mut c.checks,
                "change forms: compressed stored length < output length",
                f.length1 < f.length2,
            );
            c.zlib_in += u64::from(f.length1);
            c.zlib_out += u64::from(f.length2);
        } else {
            bump(&mut c.cf_compressed, format!("type {k:>2} stored"));
            check(
                &mut c.checks,
                "change forms: stored payload does not start with a zlib header 0x78",
                f.length1 == 0 || {
                    let mut b0 = [0u8; 1];
                    src.read_exact_at(f.data.offset, &mut b0)?;
                    b0[0] != 0x78
                },
            );
        }
        bump(
            &mut c.ref_kinds,
            format!("{:?}", f.form.kind()).to_ascii_lowercase(),
        );
        let t = s.resolve(f.form).map(|id| r.target(id));
        let cl = t.as_ref().map(class).unwrap_or("unresolved".into());
        bump(&mut c.resolution, cl.clone());
        bump(c.kind_signature.entry(k).or_default(), cl);
        for how in READINGS {
            let x = resolve_with(s, f.form, how).map(|id| r.target(id));
            let e = c.readings.entry(how.name).or_default();
            e[0] += 1;
            if let Some(Target::Record(_, Some(sig))) = x {
                e[1] += 1;
                // Agreement of the record type with the candidate reading's.
                if let Some(Target::Record(_, Some(base))) = &t {
                    if *base == sig {
                        e[2] += 1;
                    }
                }
            }
        }
    }

    // Form id array.
    let mut seen = HashSet::new();
    for id in &s.form_ids {
        bump(
            &mut c.form_ids,
            format!("form version {}: {}", s.form_version, class(&r.target(*id))),
        );
        if *id != 0 {
            check(
                &mut c.checks,
                "form id array: nonzero entries distinct",
                seen.insert(*id),
            );
        }
    }
    add(&mut c.form_ids, "~ entries", s.form_ids.len() as u64);
    let used: HashSet<usize> = s
        .change_forms
        .iter()
        .filter(|f| f.form.kind() == RefKind::Indexed)
        .map(|f| f.form.value() as usize)
        .collect();
    add(
        &mut c.form_ids,
        "~ entries indexed by a change form",
        used.len() as u64,
    );
    for i in &used {
        check(
            &mut c.checks,
            "form id array: an entry a change form indexes is not zero",
            i.checked_sub(1)
                .and_then(|i| s.form_ids.get(i))
                .is_some_and(|x| *x != 0),
        );
    }

    // Second array.
    bump(&mut c.second, format!("~ length {}", s.second_array.len()));
    for id in &s.second_array {
        bump(&mut c.second, class(&r.target(*id)));
    }

    // Closing block.
    let words: Vec<String> = s
        .closing_bytes
        .chunks(4)
        .map(|w| {
            if w.len() == 4 {
                u32::from_le_bytes([w[0], w[1], w[2], w[3]]).to_string()
            } else {
                format!("{} trailing bytes", w.len())
            }
        })
        .collect();
    bump(
        &mut c.closing,
        format!(
            "{} bytes, u32 words {}",
            s.closing_bytes.len(),
            words.join(" ")
        ),
    );
    Ok(())
}

fn distance_bucket(d: f64) -> &'static str {
    [
        (1.0, "(0, 1)"),
        (128.0, "[1, 128)"),
        (4096.0, "[128, 4096)"),
    ]
    .iter()
    .find(|(limit, _)| d < *limit)
    .map_or(
        ">= 4096",
        |(_, label)| if d <= 0.0 { "0 (equal)" } else { label },
    )
}

fn f32_at(p: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]])
}

/// The candidate layout of `save_refr_move_layout_v1`: a reference (three
/// bytes) to the cell or world the record is in, three f32 of position and
/// three of rotation, at the start of the payload of a type 0 or 1 change
/// form with flag bit 1. Checked against the placement the plugin data
/// gives the same record.
fn moved(
    c: &mut Census,
    s: &Save,
    src: &FileSource,
    l: &Limits,
    store: &Store,
    m: &Manifest,
    placed: &Placed,
) -> Result<(), Box<dyn std::error::Error>> {
    let r = Resolver::new(store, s, &m.root);
    for f in &s.change_forms {
        let k = f.kind();
        if k > 1 || f.flags & 2 == 0 {
            continue;
        }
        let t = format!("type {k}");
        bump(&mut c.moved, format!("{t}: entries"));
        let b = Budget::new(l.max_memory);
        let p = f.payload(src, l, &b)?;
        if p.len() < 27 {
            bump(&mut c.moved, format!("{t}: payload shorter than 27 bytes"));
            continue;
        }
        let space = RefId([p[0], p[1], p[2]]);
        let space_t = s.resolve(space).map(|id| r.target(id));
        bump(
            &mut c.moved,
            format!(
                "{t}: bytes 0-2 name {}",
                space_t.as_ref().map(class).unwrap_or("unresolved".into())
            ),
        );
        let pos = [f32_at(&p, 3), f32_at(&p, 7), f32_at(&p, 11)];
        let rot = [f32_at(&p, 15), f32_at(&p, 19), f32_at(&p, 23)];
        check(
            &mut c.checks,
            "moved: three position f32 finite, |x| < 2^20",
            pos.iter().all(|x| x.is_finite() && x.abs() < 1_048_576.0),
        );
        check(
            &mut c.checks,
            "moved: three rotation f32 finite, |x| <= 2 pi + 0.01",
            rot.iter()
                .all(|x| x.is_finite() && x.abs() <= std::f32::consts::TAU + 0.01),
        );
        let own = s.resolve(f.form).map(|id| r.target(id));
        let Some(Target::Record(key, Some(_))) = own else {
            bump(&mut c.moved, format!("{t}: record not in the plugin data"));
            continue;
        };
        // Plugin position: DATA, six f32.
        let rec = store.raw(&key, &b)?;
        let data = rec.as_ref().and_then(|rec| {
            rec.subrecords
                .iter()
                .find(|x| x.signature == *b"DATA")
                .map(|x| rec.data(x).to_vec())
        });
        if let Some(d) = data.filter(|d| d.len() == 24) {
            let q = [f32_at(&d, 0), f32_at(&d, 4), f32_at(&d, 8)];
            let dist = pos
                .iter()
                .zip(q)
                .map(|(a, b)| f64::from(*a - b).powi(2))
                .sum::<f64>()
                .sqrt();
            bump(
                &mut c.moved,
                format!(
                    "{t}: distance to the plugin DATA position {}",
                    distance_bucket(dist)
                ),
            );
        } else {
            bump(
                &mut c.moved,
                format!("{t}: plugin record without 24-byte DATA"),
            );
        }
        let Some(cell) = placed.get(&key) else {
            bump(
                &mut c.moved,
                format!("{t}: plugin record outside a cell group"),
            );
            continue;
        };
        let named = match &space_t {
            Some(Target::Record(k2, _)) => Some(k2),
            _ => None,
        };
        match store.world(cell) {
            None => bump(
                &mut c.moved,
                format!(
                    "{t}: plugin cell interior or outside a world: bytes 0-2 name {}",
                    if named == Some(cell) {
                        "that cell"
                    } else {
                        "another form"
                    }
                ),
            ),
            Some(world) => {
                bump(
                    &mut c.moved,
                    format!(
                        "{t}: plugin cell exterior: bytes 0-2 name {}",
                        if named == Some(&world) {
                            "its world"
                        } else if named == Some(cell) {
                            "that cell"
                        } else {
                            "another form"
                        }
                    ),
                );
                if let Some(g) = store.get::<Cell>(cell, &b)?.and_then(|x| x.data.grid) {
                    let gi = (pos[0] / 4096.0).floor() as i64;
                    let gj = (pos[1] / 4096.0).floor() as i64;
                    let d = (gi - i64::from(g.i)).abs().max((gj - i64::from(g.j)).abs());
                    bump(
                        &mut c.moved,
                        format!(
                            "{t}: exterior: position / 4096 against the plugin cell grid, Chebyshev distance {}",
                            match d {
                                0 => "0 (same cell)",
                                1 => "1",
                                2..=4 => "2-4",
                                _ => ">= 5",
                            }
                        ),
                    );
                }
            }
        }
    }
    Ok(())
}

fn write(o: &mut String, c: &Census, secs: f64) -> std::fmt::Result {
    writeln!(o, "save files\t{}", c.files)?;
    writeln!(o, "bytes\t{}", c.bytes)?;
    writeln!(o, "bytes framed by the reader\t{}", c.framed)?;
    writeln!(o, "wall seconds (plugin index included)\t{secs:.1}")?;
    writeln!(o, "\n# outcomes")?;
    for (k, v) in &c.outcomes {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(o, "\n# bytes by part (all files)")?;
    for (k, v) in &c.parts {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(o, "\n# bytes interpreted and kept raw (all files)")?;
    for (k, v) in &c.raw {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(o, "\n# checks: holds, fails")?;
    for (k, [a, b]) in &c.checks {
        writeln!(o, "{a}\t{b}\t{k}")?;
    }
    writeln!(o, "\n# widths and sizes: files, min, max")?;
    for (k, r) in &c.widths {
        writeln!(o, "{}\t{}\t{}\t{k}", r.n, r.min, r.max)?;
    }
    writeln!(o, "\n# stored values and counts")?;
    for (k, v) in &c.values {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(o, "\n# plugin lists (files)")?;
    for (k, v) in &c.plugins {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(o, "\n# global data: files, min length, max length, total")?;
    for (k, r) in &c.globals {
        writeln!(o, "{}\t{}\t{}\t{}\t{k}", r.n, r.min, r.max, r.sum)?;
    }
    writeln!(
        o,
        "\n# change forms by type (low six bits of the type byte)"
    )?;
    for (k, v) in &c.cf_kind {
        writeln!(o, "{v}\ttype {k}")?;
    }
    writeln!(
        o,
        "\n# change forms: entries, min, max, total stored length"
    )?;
    for (k, r) in &c.cf_lengths {
        writeln!(o, "{}\t{}\t{}\t{}\t{k}", r.n, r.min, r.max, r.sum)?;
    }
    writeln!(o, "\n# change forms: stored or compressed")?;
    for (k, v) in &c.cf_compressed {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "{}\t{}\tcompressed bytes stored, bytes after inflating",
        c.zlib_in, c.zlib_out
    )?;
    writeln!(o, "\n# change forms: version byte")?;
    for (k, v) in &c.cf_version {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "\n# change forms: width code (top two bits of the type byte)"
    )?;
    for (k, v) in &c.cf_width {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "\n# change forms: flag bits set, by type (bit: entries; bits never set omitted)"
    )?;
    for (k, bits) in &c.cf_bits {
        let n = c.cf_kind.get(k).copied().unwrap_or(0);
        let set: Vec<String> = bits
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > 0)
            .map(|(b, v)| format!("{b}:{v}"))
            .collect();
        writeln!(o, "type {k} ({n})\t{}", set.join(" "))?;
    }
    writeln!(o, "\n# change forms: reference kind (top two bits)")?;
    for (k, v) in &c.ref_kinds {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "\n# change forms: what the reference names (candidate reading)"
    )?;
    for (k, v) in &c.resolution {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "\n# change forms: record type named, by change-form type"
    )?;
    for (k, m) in &c.kind_signature {
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let cells: Vec<String> = v.iter().map(|(s, n)| format!("{s} {n}")).collect();
        writeln!(o, "type {k}\t{}", cells.join(", "))?;
    }
    writeln!(
        o,
        "\n# change forms under alternative readings: entries, naming a winning record, same record type as the candidate"
    )?;
    for (k, [a, b, d]) in &c.readings {
        writeln!(o, "{a}\t{b}\t{d}\t{k}")?;
    }
    writeln!(o, "\n# form id array: what each entry names")?;
    for (k, v) in &c.form_ids {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "\n# second array: lengths (files) and what each entry names"
    )?;
    for (k, v) in &c.second {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(
        o,
        "\n# moved references: change forms of types 0 and 1 with flag bit 1, candidate layout at payload byte 0"
    )?;
    for (k, v) in &c.moved {
        writeln!(o, "{v}\t{k}")?;
    }
    writeln!(o, "\n# closing block (files)")?;
    for (k, v) in &c.closing {
        writeln!(o, "{v}\t{k}")?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
