//! The record census: the subrecords of the record types that tie sounds to
//! the world, the four-byte slots that name a sound-family record, the
//! candidate links between them, how the file paths they store resolve to
//! mounted files, the placed references of sound-family bases, and what the
//! test cells place and assign.
use crate::files::Index;
use crate::{bump, check, checks, counts, fourcc, section, Checks, Counts};
use fo4_audio::pcm::Status;
use fo4_core::{path_key_bytes, Budget, Manifest};
use fo4_plugin::{typed::Store, FormKey, Record};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
};

/// Record types the census surveys subrecord by subrecord.
const SURVEY: [&[u8; 4]; 16] = [
    b"SNDR", b"SOUN", b"SNCT", b"SOPM", b"MUSC", b"MUST", b"ASPC", b"AECH", b"REVB", b"REGN",
    b"CELL", b"WRLD", b"LCTN", b"DOOR", b"ACTI", b"WEAP",
];
/// Targets counted as the sound family when a four-byte slot resolves.
const FAMILY: [&[u8; 4]; 10] = [
    b"SNDR", b"SOUN", b"SNCT", b"SOPM", b"MUSC", b"MUST", b"ASPC", b"AECH", b"REVB", b"REGN",
];
/// Largest subrecord scanned at every byte offset.
const SCAN_MAX: usize = 4096;

/// A candidate link (`audio_record_links_v1`): in records of type `from`,
/// subrecord `sub` holds four-byte FormIDs at `offset`, `offset + stride`,
/// ... naming records of type `to`; `size` restricts it to one size.
struct Link {
    from: &'static [u8; 4],
    sub: &'static [u8; 4],
    offset: usize,
    stride: usize,
    /// Largest number of slots; `None` for as many as the subrecord holds.
    count: Option<usize>,
    size: Option<usize>,
    to: &'static [u8; 4],
}
const fn l(from: &'static [u8; 4], sub: &'static [u8; 4], to: &'static [u8; 4]) -> Link {
    Link {
        from,
        sub,
        offset: 0,
        stride: 4,
        count: None,
        size: None,
        to,
    }
}
impl Link {
    /// Byte offsets of the slots in a subrecord of `len` bytes.
    fn slots(&self, len: usize) -> impl Iterator<Item = usize> + '_ {
        (self.offset..len.saturating_sub(3))
            .step_by(self.stride)
            .take(self.count.unwrap_or(usize::MAX))
    }
    fn name(&self) -> String {
        format!(
            "{} {} from {} every {} bytes{}{} -> {}",
            fourcc(self.from),
            fourcc(self.sub),
            self.offset,
            self.stride,
            self.count.map_or(String::new(), |n| format!(", {n} slots")),
            self.size
                .map_or(String::new(), |n| format!(", size {n} only")),
            fourcc(self.to)
        )
    }
}
const LINKS: &[Link] = &[
    l(b"CELL", b"XCAS", b"ASPC"),
    l(b"CELL", b"XCMO", b"MUSC"),
    l(b"CELL", b"XCCM", b"REGN"),
    l(b"CELL", b"XCLR", b"REGN"),
    l(b"CELL", b"XLCN", b"LCTN"),
    l(b"LCTN", b"NAM1", b"MUSC"),
    l(b"WRLD", b"ZNAM", b"MUSC"),
    l(b"REGN", b"RDMO", b"MUSC"),
    Link {
        stride: 12,
        ..l(b"REGN", b"RDSA", b"SNDR")
    },
    l(b"SOUN", b"SDSC", b"SNDR"),
    l(b"ASPC", b"SNAM", b"SNDR"),
    l(b"ASPC", b"BNAM", b"REVB"),
    l(b"ASPC", b"RDAT", b"REGN"),
    l(b"DOOR", b"SNAM", b"SNDR"),
    l(b"DOOR", b"ANAM", b"SNDR"),
    l(b"ACTI", b"SNAM", b"SNDR"),
    l(b"ACTI", b"VNAM", b"SNDR"),
    Link {
        count: Some(1),
        ..l(b"ACTI", b"RADR", b"SOPM")
    },
    Link {
        offset: 73,
        count: Some(8),
        size: Some(132),
        ..l(b"WEAP", b"DNAM", b"SNDR")
    },
    l(b"WEAP", b"YNAM", b"SNDR"),
    l(b"WEAP", b"ZNAM", b"SNDR"),
    l(b"MUSC", b"TNAM", b"MUST"),
    l(b"MUST", b"SNAM", b"MUST"),
    l(b"SNDR", b"DNAM", b"SNDR"),
    l(b"SNDR", b"SNAM", b"SNDR"),
    Link {
        size: Some(4),
        ..l(b"SNDR", b"BNAM", b"SNDR")
    },
    l(b"SNDR", b"GNAM", b"SNCT"),
    l(b"SNDR", b"ONAM", b"SOPM"),
];
/// Extensions that mark a text subrecord as a sound file path.
const SOUND_EXT: [&str; 3] = ["wav", "xwm", "fuz"];
/// Path readings (`audio_record_path_readings_v1`): a prefix and an
/// extension substitution, tried in this order.
/// A prefix of `-` removes a leading `data` folder (any case, either
/// separator) instead of adding one.
const PREFIXES: [&str; 4] = ["", "-", "sound/", "music/"];
const EXTS: [Option<&str>; 3] = [None, Some("xwm"), Some("fuz")];

fn reading_name(p: usize, e: usize) -> String {
    format!(
        "{} extension {}",
        match PREFIXES[p] {
            "" => "path as stored,".to_string(),
            "-" => "leading data folder removed,".to_string(),
            x => format!("prefix '{x}',"),
        },
        EXTS[e].map_or("as stored".to_string(), |x| format!(".{x}"))
    )
}

/// The text of a zero-terminated printable subrecord. Four-byte subrecords
/// are never text here: a FormID can read as three printable bytes and a
/// zero.
fn text(d: &[u8]) -> Option<&[u8]> {
    if d.len() == 4 {
        return None;
    }
    let (last, body) = d.split_last()?;
    (*last == 0 && !body.is_empty() && body.iter().all(|c| (0x20..0x7f).contains(c)))
        .then_some(body)
}

fn extension(t: &[u8]) -> Option<String> {
    let name = t.rsplit(|b| *b == b'\\' || *b == b'/').next()?;
    let i = name.iter().rposition(|b| *b == b'.')?;
    Some(String::from_utf8_lossy(&name[i + 1..]).to_ascii_lowercase())
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

struct Ctx<'a> {
    store: &'a Store,
    budget_max: u64,
}

impl Ctx<'_> {
    fn resolve(&self, plugin: usize, id: u32, r: &Record, off: u64) -> Option<(FormKey, [u8; 4])> {
        if id == 0 {
            return None;
        }
        let k = self
            .store
            .order()
            .remap(plugin, id, &r.source.file, off)
            .ok()??;
        let w = self.store.overrides().winner(&k)?;
        if w.deleted {
            return None;
        }
        Some((k, w.signature))
    }
    fn record(&self, k: &FormKey) -> Option<(Record, usize)> {
        self.store
            .raw_with_plugin(k, &Budget::new(self.budget_max))
            .ok()?
    }
    /// The targets of every link out of a record, with the link's index.
    fn links(&self, r: &Record, plugin: usize) -> Vec<(usize, FormKey, bool)> {
        let mut out = Vec::new();
        for (li, link) in LINKS.iter().enumerate() {
            if &r.signature != link.from {
                continue;
            }
            for s in r.subrecords.iter().filter(|s| &s.signature == link.sub) {
                let d = r.data(s);
                if link.size.is_some_and(|n| n != d.len()) {
                    continue;
                }
                for o in link.slots(d.len()) {
                    let Some(id) = u32_at(d, o) else { break };
                    if let Some((k, sig)) = self.resolve(plugin, id, r, s.offset) {
                        out.push((li, k, &sig == link.to));
                    }
                }
            }
        }
        out
    }
    fn edid(r: &Record) -> String {
        r.subrecords
            .iter()
            .find(|s| &s.signature == b"EDID")
            .and_then(|s| text(r.data(s)))
            .map(|t| String::from_utf8_lossy(t).into_owned())
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct Field {
    records: u64,
    occ: u64,
    max_per_record: u64,
    sizes: BTreeMap<usize, u64>,
    text: u64,
    exts: Counts,
}

/// How one stored path resolves under the readings.
struct Resolution {
    /// Index into PREFIXES x EXTS of each reading that names a mounted file.
    hits: Vec<(usize, usize)>,
    /// Key of the first reading that names a mounted file.
    first: Option<Vec<u8>>,
}

fn resolve_path(t: &[u8], idx: &Index) -> Resolution {
    let mut hits = Vec::new();
    let mut first = None;
    for (e, x) in EXTS.iter().enumerate() {
        for (p, pre) in PREFIXES.iter().enumerate() {
            let (mut cand, t) = match *pre {
                "-" => {
                    let lower = t.to_ascii_lowercase();
                    match lower.strip_prefix(b"data") {
                        Some(r) if r.first().is_some_and(|c| *c == b'\\' || *c == b'/') => {
                            (Vec::new(), &t[5..])
                        }
                        _ => continue,
                    }
                }
                _ => (pre.as_bytes().to_vec(), t),
            };
            match x {
                None => cand.extend_from_slice(t),
                Some(x) => {
                    let stem = match t.iter().rposition(|b| *b == b'.') {
                        Some(i) if !t[i..].contains(&b'\\') && !t[i..].contains(&b'/') => &t[..i],
                        _ => t,
                    };
                    cand.extend_from_slice(stem);
                    cand.push(b'.');
                    cand.extend_from_slice(x.as_bytes());
                }
            }
            let Ok(k) = path_key_bytes(&cand) else {
                continue;
            };
            if idx.keys.contains(&k) {
                hits.push((p, e));
                if first.is_none() {
                    first = Some(k);
                }
            }
        }
    }
    Resolution { hits, first }
}

fn file_label(idx: &Index, key: &[u8]) -> String {
    match idx.sounds.get(key) {
        None => "mounted, not a sound container".into(),
        Some(i) if !i.read => format!("{} refused", i.container.label()),
        Some(i) => format!(
            "{} {}",
            i.container.label(),
            match (i.status, i.decoded) {
                (_, true) => "decoded to PCM",
                (Some(Status::WmaOpen), _) => "WMA, payload raw",
                (Some(Status::Pcm), false) => "PCM, decode refused",
                _ => "not decoded",
            }
        ),
    }
}

#[derive(Default)]
struct PathStats {
    occ: u64,
    distinct: HashSet<Vec<u8>>,
    no_ext: u64,
    exts: Counts,
    readings: Counts,
    any: u64,
    first: Counts,
    files: Counts,
}

fn put_path(ps: &mut PathStats, t: &[u8], idx: &Index) -> Resolution {
    ps.occ += 1;
    ps.distinct.insert(t.to_ascii_lowercase());
    match extension(t) {
        Some(e) => bump(&mut ps.exts, e),
        None => ps.no_ext += 1,
    }
    let r = resolve_path(t, idx);
    for (p, e) in &r.hits {
        bump(&mut ps.readings, reading_name(*p, *e));
    }
    match (r.hits.first(), &r.first) {
        (Some((p, e)), Some(k)) => {
            ps.any += 1;
            bump(&mut ps.first, reading_name(*p, *e));
            bump(&mut ps.files, file_label(idx, k));
        }
        _ => bump(&mut ps.first, "no reading names a mounted file"),
    }
    r
}

fn print_paths(out: &mut String, title: &str, ps: &PathStats) {
    section(out, title);
    let _ = writeln!(
        out,
        "occurrences\t{}\ndistinct paths (case folded)\t{}\nwithout extension\t{}\nresolving under some reading\t{}",
        ps.occ,
        ps.distinct.len(),
        ps.no_ext,
        ps.any
    );
    for (k, v) in &ps.exts {
        let _ = writeln!(out, "stored extension {k}\t{v}");
    }
    for (k, v) in &ps.readings {
        let _ = writeln!(out, "resolves under {k}\t{v}");
    }
    for (k, v) in &ps.first {
        let _ = writeln!(out, "first reading: {k}\t{v}");
    }
    for (k, v) in &ps.files {
        let _ = writeln!(out, "file named by the first reading: {k}\t{v}");
    }
}

pub fn run(
    m: &Manifest,
    store: &Store,
    idx: &Index,
    cells: &[(String, u32)],
    out: &mut String,
    names: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let cx = Ctx {
        store,
        budget_max: m.limits.max_memory,
    };
    section(out, "records: winning records by type (live, deleted)");
    let mut fields: BTreeMap<([u8; 4], [u8; 4]), Field> = BTreeMap::new();
    // (type, sub, offset, aligned, target) -> hits
    let mut slots: BTreeMap<(String, String, usize, String), u64> = BTreeMap::new();
    let mut link_checks = Checks::new();
    let mut link_counts: BTreeMap<usize, [u64; 5]> = BTreeMap::new();
    let mut slot_counts: BTreeMap<(usize, usize), [u64; 5]> = BTreeMap::new();
    let mut paths: BTreeMap<String, PathStats> = BTreeMap::new();
    let mut sounding_bases: HashMap<FormKey, [u8; 4]> = HashMap::new();
    for sig in SURVEY {
        let keys: Vec<FormKey> = store
            .winners()
            .filter(|w| &w.signature == sig)
            .map(|w| store.overrides().form_key(w.key, store.order()))
            .collect();
        let mut live = 0u64;
        let mut deleted = 0u64;
        for k in &keys {
            let Some((r, plugin)) = cx.record(k) else {
                continue;
            };
            if r.deleted() {
                deleted += 1;
                continue;
            }
            live += 1;
            let mut per: BTreeMap<[u8; 4], u64> = BTreeMap::new();
            for s in &r.subrecords {
                *per.entry(s.signature).or_default() += 1;
                let d = r.data(s);
                let f = fields.entry((*sig, s.signature)).or_default();
                f.occ += 1;
                *f.sizes.entry(d.len()).or_default() += 1;
                if let Some(t) = text(d) {
                    f.text += 1;
                    let e = extension(t).unwrap_or_else(|| "(none)".into());
                    if SOUND_EXT.contains(&e.as_str()) {
                        let ps = paths
                            .entry(format!("{} {}", fourcc(sig), fourcc(&s.signature)))
                            .or_default();
                        put_path(ps, t, idx);
                    }
                    bump(&mut f.exts, e);
                    continue;
                }
                if d.len() < 4 || d.len() > SCAN_MAX {
                    continue;
                }
                for o in 0..=d.len() - 4 {
                    let id = u32_at(d, o).unwrap_or_default();
                    if let Some((_, t)) = cx.resolve(plugin, id, &r, s.offset) {
                        if FAMILY.contains(&&t) {
                            *slots
                                .entry((fourcc(sig), fourcc(&s.signature), o, fourcc(&t)))
                                .or_default() += 1;
                        }
                    }
                }
            }
            for (s, n) in per {
                let f = fields.get_mut(&(*sig, s)).expect("field");
                f.records += 1;
                f.max_per_record = f.max_per_record.max(n);
            }
            for (li, link) in LINKS.iter().enumerate() {
                if &r.signature != link.from {
                    continue;
                }
                for s in r.subrecords.iter().filter(|s| &s.signature == link.sub) {
                    let d = r.data(s);
                    link_counts.entry(li).or_default()[0] += 1;
                    if link.size.is_some_and(|n| n != d.len()) {
                        continue;
                    }
                    let name = link.name();
                    if link.count.is_none() {
                        check(
                            &mut link_checks,
                            format!("{name}: size minus offset a multiple of the stride"),
                            d.len() > link.offset && (d.len() - link.offset) % link.stride == 0,
                        );
                    }
                    for (slot, o) in link.slots(d.len()).enumerate() {
                        let Some(id) = u32_at(d, o) else { break };
                        let col = if id == 0 {
                            1
                        } else {
                            match cx.resolve(plugin, id, &r, s.offset) {
                                Some((_, t)) if &t == link.to => 2,
                                Some(_) => 3,
                                None => 4,
                            }
                        };
                        link_counts.entry(li).or_default()[col] += 1;
                        if link.count.is_some() {
                            slot_counts.entry((li, slot)).or_default()[col] += 1;
                        }
                    }
                }
            }
            if matches!(sig, b"DOOR" | b"ACTI" | b"SOUN" | b"ASPC")
                && (matches!(sig, b"SOUN" | b"ASPC")
                    || cx.links(&r, plugin).iter().any(|(_, _, ok)| *ok))
            {
                sounding_bases.insert(k.clone(), *sig);
            }
        }
        let _ = writeln!(out, "{}\t{live}\t{deleted}", fourcc(sig));
    }
    section(
        out,
        "subrecords: type, subrecord, records with it, occurrences, most per record, sizes (count) up to 8 most common, distinct sizes, text occurrences, text extensions",
    );
    for ((sig, sub), f) in &fields {
        let mut sizes: Vec<(&usize, &u64)> = f.sizes.iter().collect();
        sizes.sort_by_key(|(s, n)| (std::cmp::Reverse(**n), **s));
        let top: Vec<String> = sizes
            .iter()
            .take(8)
            .map(|(s, n)| format!("{s}:{n}"))
            .collect();
        let exts: Vec<String> = f.exts.iter().map(|(e, n)| format!("{e}:{n}")).collect();
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            fourcc(sig),
            fourcc(sub),
            f.records,
            f.occ,
            f.max_per_record,
            top.join(","),
            f.sizes.len(),
            f.text,
            exts.join(",")
        );
    }
    section(
        out,
        "four-byte values that name a live sound-family record: type, subrecord, byte offset, alignment, target type, values, occurrences of the subrecord (offsets where at least 2 values and 5% of the occurrences do)",
    );
    let (mut below_rows, mut below_values) = (0u64, 0u64);
    for ((sig, sub, o, t), n) in &slots {
        let occ = fields
            .iter()
            .find(|((a, b), _)| fourcc(a) == *sig && fourcc(b) == *sub)
            .map_or(0, |(_, f)| f.occ);
        if *n < 2 || n * 20 < occ {
            below_rows += 1;
            below_values += n;
            continue;
        }
        let _ = writeln!(
            out,
            "{sig}\t{sub}\t{o}\t{}\t{t}\t{n}\t{occ}",
            if o % 4 == 0 { "aligned" } else { "unaligned" }
        );
    }
    let _ = writeln!(
        out,
        "offsets below the threshold\t{below_rows}\nvalues at them\t{below_values}"
    );
    section(
        out,
        "candidate links (audio_record_links_v1): subrecords, slots zero, slots naming the target type, slots naming another type, slots naming no live record",
    );
    for (li, link) in LINKS.iter().enumerate() {
        let r = link_counts.get(&li).copied().unwrap_or_default();
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}",
            link.name(),
            r[0],
            r[1],
            r[2],
            r[3],
            r[4]
        );
    }
    for ((li, slot), r) in &slot_counts {
        let link = &LINKS[*li];
        let _ = writeln!(
            out,
            "{} slot {slot} at +{}\t\t{}\t{}\t{}\t{}",
            link.name(),
            link.offset + slot * link.stride,
            r[1],
            r[2],
            r[3],
            r[4]
        );
    }
    checks(out, "candidate links", &link_checks);
    for (name, ps) in &paths {
        print_paths(
            out,
            &format!("file paths in {name} (audio_record_path_readings_v1)"),
            ps,
        );
    }

    placed(&cx, out, &sounding_bases)?;
    for (n, (cell, radius)) in cells.iter().enumerate() {
        test_cell(&cx, idx, n + 1, cell, *radius, out, names)?;
    }
    Ok(())
}

/// Every winning placed REFR of the game by the type of its base, and the
/// four-byte values in placed REFRs that name sound-family records.
fn placed(
    cx: &Ctx<'_>,
    out: &mut String,
    sounding: &HashMap<FormKey, [u8; 4]>,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = cx.store;
    let mut by_base = Counts::new();
    let mut subs = Counts::new();
    let mut total = 0u64;
    let mut unread = 0u64;
    for (o, _) in store.placement_cells() {
        if o.signature != *b"REFR" || o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let Some((r, plugin)) = cx.record(&k) else {
            unread += 1;
            continue;
        };
        total += 1;
        let base = r
            .subrecords
            .iter()
            .find(|s| &s.signature == b"NAME")
            .and_then(|s| u32_at(r.data(s), 0).map(|id| (id, s.offset)))
            .and_then(|(id, off)| cx.resolve(plugin, id, &r, off));
        if let Some((bk, sig)) = &base {
            if FAMILY.contains(&sig) {
                bump(&mut by_base, format!("base {}", fourcc(sig)));
            }
            if matches!(sig, b"DOOR" | b"ACTI") {
                bump(
                    &mut by_base,
                    format!(
                        "base {} {}",
                        fourcc(sig),
                        if sounding.contains_key(bk) {
                            "with a sound link"
                        } else {
                            "without a sound link"
                        }
                    ),
                );
            }
        } else {
            bump(&mut by_base, "base not resolved");
        }
        for s in &r.subrecords {
            if s.signature == *b"NAME" {
                continue;
            }
            let d = r.data(s);
            for o in (0..d.len().min(SCAN_MAX)).step_by(4) {
                let Some(id) = u32_at(d, o) else { break };
                if let Some((_, t)) = cx.resolve(plugin, id, &r, s.offset) {
                    if FAMILY.contains(&&t) {
                        bump(
                            &mut subs,
                            format!("{} +{o} -> {}", fourcc(&s.signature), fourcc(&t)),
                        );
                    }
                }
            }
        }
    }
    section(out, "placed REFR (every winning, live placement)");
    let _ = writeln!(out, "placed REFR read\t{total}\nnot read\t{unread}");
    counts(
        out,
        "placed REFR by base type (sound family, doors, activators)",
        &by_base,
    );
    counts(
        out,
        "placed REFR subrecords other than NAME with an aligned value naming a sound-family record",
        &subs,
    );
    Ok(())
}

/// Where a walk starts: what the cell places or assigns.
const ORIGINS: [&str; 9] = [
    "placed SOUN (sound emitters)",
    "placed ASPC (acoustic spaces)",
    "placed DOOR (door sounds)",
    "placed ACTI (activator sounds)",
    "cell XCAS (acoustic space)",
    "cell XCMO (music type)",
    "cell XCLR and XCCM (regions)",
    "cell XLCN (location)",
    "world of an exterior cell",
];

/// The records reached from one origin by following the candidate links,
/// and the file paths they store.
#[derive(Default)]
struct Walk {
    seeds: u64,
    seen: BTreeSet<(String, u32)>,
    by_type: BTreeMap<[u8; 4], u64>,
    via: Counts,
    paths: BTreeMap<String, PathStats>,
    sndr: Counts,
}

fn walk(cx: &Ctx<'_>, idx: &Index, seeds: &[FormKey], names: &mut String) -> Walk {
    let mut w = Walk {
        seeds: seeds.len() as u64,
        ..Walk::default()
    };
    let mut queue: Vec<FormKey> = Vec::new();
    for k in seeds {
        if w.seen.insert((k.plugin.to_ascii_lowercase(), k.object_id)) {
            queue.push(k.clone());
        }
    }
    while let Some(k) = queue.pop() {
        let Some((r, plugin)) = cx.record(&k) else {
            continue;
        };
        *w.by_type.entry(r.signature).or_default() += 1;
        let _ = writeln!(names, "{}\t{}", fourcc(&r.signature), Ctx::edid(&r));
        for (li, t, ok) in cx.links(&r, plugin) {
            if ok {
                bump(&mut w.via, LINKS[li].name());
                if w.seen.insert((t.plugin.to_ascii_lowercase(), t.object_id)) {
                    queue.push(t);
                }
            }
        }
        let (mut stored, mut resolved, mut decoded) = (0u64, 0u64, 0u64);
        for s in &r.subrecords {
            let Some(t) = text(r.data(s)) else { continue };
            if !extension(t).is_some_and(|e| SOUND_EXT.contains(&e.as_str())) {
                continue;
            }
            stored += 1;
            let ps = w
                .paths
                .entry(format!("{} {}", fourcc(&r.signature), fourcc(&s.signature)))
                .or_default();
            let res = put_path(ps, t, idx);
            if let Some(f) = &res.first {
                resolved += 1;
                if idx.sounds.get(f).is_some_and(|i| i.decoded) {
                    decoded += 1;
                }
            }
            let _ = writeln!(
                names,
                "\tpath\t{}\t{}",
                String::from_utf8_lossy(t),
                res.first
                    .as_ref()
                    .map_or("unresolved".to_string(), |f| file_label(idx, f))
            );
        }
        if r.signature == *b"SNDR" {
            bump(
                &mut w.sndr,
                match (stored, resolved, decoded) {
                    (0, _, _) => "SNDR storing no file path",
                    (_, 0, _) => "SNDR whose paths all fail to resolve",
                    (_, _, 0) => "SNDR with a resolved file, none decoded",
                    _ => "SNDR with at least one file decoded to PCM",
                },
            );
        }
    }
    w
}

fn base_of(cx: &Ctx<'_>, r: &Record, plugin: usize) -> Option<(FormKey, [u8; 4])> {
    r.subrecords
        .iter()
        .find(|s| &s.signature == b"NAME")
        .and_then(|s| u32_at(r.data(s), 0).map(|id| (id, s.offset)))
        .and_then(|(id, off)| cx.resolve(plugin, id, r, off))
}

#[allow(clippy::too_many_arguments)]
fn test_cell(
    cx: &Ctx<'_>,
    idx: &Index,
    n: usize,
    name: &str,
    radius: u32,
    out: &mut String,
    names: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = cx.store;
    let b = Budget::new(cx.budget_max);
    let Some(cell) = store.find_cell(name, &b)? else {
        section(out, &format!("test cell {n}: not found"));
        return Ok(());
    };
    let around = fo4_scene::grid::around(store, &cell, radius, &b)?;
    let mut list = vec![cell.clone()];
    list.extend(around.neighbours.iter().filter_map(|x| x.cell.clone()));
    let interior = store.world(&cell).is_none();
    let _ = writeln!(names, "\n# test cell {n}: {name} radius {radius}");
    let mut c = Counts::new();
    let mut seeds: BTreeMap<usize, Vec<FormKey>> = BTreeMap::new();
    let mut worlds = BTreeSet::new();
    for ck in &list {
        let Some((cr, cp)) = cx.record(ck) else {
            continue;
        };
        bump(&mut c, "cells");
        for (li, k, ok) in cx.links(&cr, cp) {
            let link = &LINKS[li];
            bump(
                &mut c,
                format!(
                    "cell {} -> {}{}",
                    fourcc(link.sub),
                    fourcc(link.to),
                    if ok { "" } else { " (other type)" }
                ),
            );
            let origin = match link.sub {
                b"XCAS" => 4,
                b"XCMO" => 5,
                b"XCLR" | b"XCCM" => 6,
                _ => 7,
            };
            if ok {
                seeds.entry(origin).or_default().push(k);
            }
        }
        if let Some(w) = store.world(ck) {
            worlds.insert(w);
        }
        let ch = store.children(ck, &b)?;
        for (state, kids) in [
            ("active", &ch.active),
            ("initially disabled", &ch.disabled),
            ("other group or type", &ch.excluded),
        ] {
            for kid in kids.iter().filter(|x| x.signature == *b"REFR") {
                bump(&mut c, format!("placed REFR ({state})"));
                let Some((r, plugin)) = cx.record(&kid.key) else {
                    continue;
                };
                let Some((bk, sig)) = base_of(cx, &r, plugin) else {
                    bump(&mut c, "placed REFR without a resolved base");
                    continue;
                };
                let origin = match &sig {
                    b"SOUN" => Some(0),
                    b"ASPC" => Some(1),
                    b"DOOR" => Some(2),
                    b"ACTI" => Some(3),
                    _ => None,
                };
                if FAMILY.contains(&&sig) || origin.is_some() {
                    bump(
                        &mut c,
                        format!("placed REFR of a {} base ({state})", fourcc(&sig)),
                    );
                }
                if let Some(o) = origin {
                    if o >= 2 {
                        let sounds = cx
                            .record(&bk)
                            .map(|(br, bp)| cx.links(&br, bp))
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|(_, _, ok)| *ok)
                            .count();
                        bump(
                            &mut c,
                            format!(
                                "placed {} whose base has {} sound link(s)",
                                fourcc(&sig),
                                sounds
                            ),
                        );
                        if sounds == 0 {
                            continue;
                        }
                    }
                    seeds.entry(o).or_default().push(bk);
                }
                for s in &r.subrecords {
                    if s.signature == *b"NAME" {
                        continue;
                    }
                    let d = r.data(s);
                    for o in (0..d.len().min(SCAN_MAX)).step_by(4) {
                        let Some(id) = u32_at(d, o) else { break };
                        if let Some((_, t)) = cx.resolve(plugin, id, &r, s.offset) {
                            if FAMILY.contains(&&t) {
                                bump(
                                    &mut c,
                                    format!(
                                        "placed REFR value naming a {} in {} +{o}",
                                        fourcc(&t),
                                        fourcc(&s.signature)
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for w in &worlds {
        if let Some((wr, wp)) = cx.record(w) {
            for (li, k, ok) in cx.links(&wr, wp) {
                if ok {
                    bump(
                        &mut c,
                        format!(
                            "world {} -> {}",
                            fourcc(LINKS[li].sub),
                            fourcc(LINKS[li].to)
                        ),
                    );
                    seeds.entry(8).or_default().push(k);
                }
            }
        }
    }
    section(
        out,
        &format!(
            "test cell {n} ({}, radius {radius}, {} cells)",
            if interior { "interior" } else { "exterior" },
            list.len()
        ),
    );
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    for (o, keys) in &seeds {
        let _ = writeln!(names, "## origin {}", ORIGINS[*o]);
        let w = walk(cx, idx, keys, names);
        section(out, &format!("test cell {n}, from {}", ORIGINS[*o]));
        let _ = writeln!(
            out,
            "seeds (with repeats)\t{}\ndistinct records reached\t{}",
            w.seeds,
            w.seen.len()
        );
        for (t, k) in &w.by_type {
            let _ = writeln!(out, "{k}\t{} reached", fourcc(t));
        }
        for (k, v) in &w.via {
            let _ = writeln!(out, "{v}\tlink followed: {k}");
        }
        for (k, v) in &w.sndr {
            let _ = writeln!(out, "{v}\t{k}");
        }
        for (field, ps) in &w.paths {
            print_paths(
                out,
                &format!("test cell {n}, from {}: paths in {field}", ORIGINS[*o]),
                ps,
            );
        }
    }
    Ok(())
}
