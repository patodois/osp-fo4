//! The file census: every archive entry by extension and by first bytes,
//! the loose files under the data root, and every sound container read to
//! its last byte with its header fields, chunks and codec.
use crate::{add, bump, check, checks, counts, fourcc, section, Checks, Counts, Range};
use fo4_audio::{
    format::{tag_name, TAG_PCM},
    packets,
    pcm::Status,
    wave::{self, Cue, Smpl},
    Container, Parts, Sound,
};
use fo4_core::{Budget, Manifest};
use fo4_vfs::{Location, Vfs};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
    path::Path,
};

/// What the census learned about one winning sound file.
#[derive(Clone, Copy)]
pub struct Info {
    pub container: Container,
    /// Read to the last byte.
    pub read: bool,
    pub status: Option<Status>,
    /// Decoded to PCM by this crate.
    pub decoded: bool,
}

/// Every mounted path (normalized key) and the winning sound files.
pub struct Index {
    pub keys: HashSet<Vec<u8>>,
    pub sounds: HashMap<Vec<u8>, Info>,
}

fn group(vfs: &Vfs, loc: &Location) -> &'static str {
    match loc {
        Location::Archive { archive, .. } => {
            let p = vfs.archives[*archive].path.to_ascii_lowercase();
            if p.starts_with("fallout4 - ") {
                "base"
            } else if p.starts_with("dlc") {
                "dlc"
            } else {
                "other"
            }
        }
        Location::Loose { .. } => "loose",
    }
}

fn ext(key: &[u8]) -> String {
    let name = key.rsplit(|b| *b == b'/').next().unwrap_or(key);
    match name.iter().rposition(|b| *b == b'.') {
        Some(i) => String::from_utf8_lossy(&name[i + 1..]).into_owned(),
        None => "(none)".into(),
    }
}

/// Size class for histograms: powers of two.
fn pow2(n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let p = 63 - n.leading_zeros();
    format!("2^{p}..2^{}", p + 1)
}

#[derive(Default)]
struct Stats {
    files: u64,
    bytes: u64,
    outcomes: Counts,
    parts: Parts,
    formats: Counts,
    groups: Counts,
    avg: Counts,
    chunk_files: Counts,
    chunk_occ: Counts,
    chunk_bytes: Counts,
    chunk_sizes: BTreeMap<String, Range>,
    orders: Counts,
    pads: Counts,
    checks: Checks,
    smpl: Counts,
    cue: Counts,
    list: Counts,
    decode: Counts,
    frames: u64,
    pcm_seconds: f64,
    pcm_bytes: u64,
    pcm_files: u64,
    xwma: Counts,
    dpds_steps: Counts,
    dpds_seconds: f64,
    packets: u64,
    fuz: Counts,
    lip_sizes: Counts,
    lip: Range,
    durations: Counts,
}

fn seconds_class(s: f64) -> &'static str {
    match s {
        x if x < 1.0 => "< 1 s",
        x if x < 5.0 => "1-5 s",
        x if x < 30.0 => "5-30 s",
        x if x < 120.0 => "30-120 s",
        _ => ">= 120 s",
    }
}

/// Sum of the absolute steps between consecutive samples of each channel;
/// `None` when every step is zero.
fn roughness(samples: &[i32], channels: usize) -> Option<f64> {
    let step: f64 = samples
        .iter()
        .zip(samples.iter().skip(channels))
        .map(|(a, b)| f64::from((b - a).unsigned_abs()))
        .sum();
    (step > 0.0).then_some(step)
}

fn add_parts(a: &mut Parts, p: &Parts) {
    a.fuz_header += p.fuz_header;
    a.lip += p.lip;
    a.framing += p.framing;
    a.format += p.format;
    a.seek += p.seek;
    a.data += p.data;
    a.other += p.other;
}

fn census_file(st: &mut Stats, s: &Sound, bytes: &[u8], b: &Budget) -> (bool, Option<Status>) {
    add_parts(&mut st.parts, &s.parts);
    let w = &s.wave;
    let f = &w.format;
    let audio = s.audio_offset as usize;
    let inner = &bytes[audio..];
    let tag = tag_name(f.tag).unwrap_or("other");
    bump(
        &mut st.formats,
        format!(
            "form {} tag {:#06x} ({tag}) channels {} rate {} bits {} align {} extension {}",
            fourcc(&w.riff.form),
            f.tag,
            f.channels,
            f.sample_rate,
            f.bits,
            f.block_align,
            f.extension
                .as_ref()
                .map_or("none".to_string(), |e| format!("{} bytes", e.len()))
        ),
    );
    let ids: Vec<String> = w.riff.chunks.iter().map(|c| fourcc(&c.id)).collect();
    bump(&mut st.orders, ids.join(","));
    check(
        &mut st.checks,
        "fmt chunk before data chunk",
        w.format_chunk < w.data_chunk,
    );
    check(
        &mut st.checks,
        "fmt chunk is the first chunk",
        w.format_chunk == 0,
    );
    let mut seen: HashSet<[u8; 4]> = HashSet::new();
    for c in &w.riff.chunks {
        let id = fourcc(&c.id);
        if seen.insert(c.id) {
            bump(&mut st.chunk_files, id.clone());
        }
        bump(&mut st.chunk_occ, id.clone());
        add(&mut st.chunk_bytes, id.clone(), c.data.length);
        st.chunk_sizes
            .entry(id.clone())
            .or_default()
            .put(c.data.length);
        if let Some(p) = c.pad {
            bump(
                &mut st.pads,
                format!("{id} pad byte {}", if p == 0 { "zero" } else { "non-zero" }),
            );
        }
        let d = c.data.of(inner).unwrap_or_default();
        match &c.id {
            b"smpl" => match Smpl::parse(d) {
                None => bump(&mut st.smpl, "candidate layout does not frame the chunk"),
                Some(sm) => {
                    bump(&mut st.smpl, "candidate layout frames the chunk");
                    bump(&mut st.smpl, format!("loops {}", sm.loops.len()));
                    bump(
                        &mut st.smpl,
                        format!("trailing bytes {}", sm.trailing.len()),
                    );
                    for (i, h) in sm.header.iter().enumerate().take(7) {
                        if i != 2 && i != 3 {
                            bump(&mut st.smpl, format!("header word {i} value {h}"));
                        }
                    }
                    bump(
                        &mut st.smpl,
                        format!("header word 3 value {}", sm.header[3]),
                    );
                    let period =
                        (1_000_000_000f64 / f64::from(f.sample_rate.max(1))).floor() as u32;
                    check(
                        &mut st.checks,
                        "smpl word 2 == floor(1e9 / sample rate) (candidate: sample period in ns)",
                        sm.header[2] == period || sm.header[2] == period + 1,
                    );
                    let frames = if f.tag == TAG_PCM && f.block_align > 0 {
                        Some(w.data.length / u64::from(f.block_align))
                    } else {
                        None
                    };
                    for l in &sm.loops {
                        bump(&mut st.smpl, format!("loop kind {}", l.kind));
                        bump(&mut st.smpl, format!("loop play count {}", l.play_count));
                        bump(&mut st.smpl, format!("loop fraction {}", l.fraction));
                        check(&mut st.checks, "smpl loop start <= end", l.start <= l.end);
                        if let Some(n) = frames {
                            check(
                                &mut st.checks,
                                "smpl loop end < frames of data (candidate: inclusive frame index)",
                                u64::from(l.end) < n,
                            );
                            check(
                                &mut st.checks,
                                "smpl loop end == frames of data - 1",
                                u64::from(l.end) + 1 == n,
                            );
                        }
                    }
                }
            },
            b"cue " => match Cue::parse(d) {
                None => bump(&mut st.cue, "candidate layout does not frame the chunk"),
                Some(cu) => {
                    bump(&mut st.cue, "candidate layout frames the chunk");
                    bump(&mut st.cue, format!("points {}", cu.points.len()));
                }
            },
            b"LIST" => match wave::list(d, b) {
                None => bump(&mut st.list, "candidate layout does not frame the chunk"),
                Some((kind, sub)) => {
                    bump(&mut st.list, "candidate layout frames the chunk");
                    bump(&mut st.list, format!("list type {}", fourcc(&kind)));
                    for x in &sub {
                        bump(
                            &mut st.list,
                            format!("list type {} sub-chunk {}", fourcc(&kind), fourcc(&x.id)),
                        );
                    }
                }
            },
            _ => {}
        }
    }
    let status = s.status();
    let mut decoded = false;
    match status {
        Status::Pcm => {
            check(
                &mut st.checks,
                "PCM: average bytes per second == rate * block align",
                u64::from(f.avg_bytes_per_sec)
                    == u64::from(f.sample_rate) * u64::from(f.block_align),
            );
            match s.decode(bytes, "sound", b) {
                Ok(p) => {
                    decoded = true;
                    bump(&mut st.decode, "decoded to PCM");
                    if let Some(d) = s.data(bytes) {
                        let width = usize::from(f.bits / 8);
                        let ch = usize::from(f.channels);
                        let le = roughness(&p.samples, ch);
                        let be: Vec<i32> = d
                            .chunks_exact(width)
                            .map(|x| {
                                let mut v = [0u8; 4];
                                for (i, b) in x.iter().rev().enumerate() {
                                    v[4 - width + i] = *b;
                                }
                                i32::from_le_bytes(v) >> (8 * (4 - width))
                            })
                            .collect();
                        let top = 1i32 << (f.bits - 1);
                        let offset: Vec<i32> = p
                            .samples
                            .iter()
                            .map(|x| if *x < 0 { x + top } else { x - top })
                            .collect();
                        if le.is_none() {
                            bump(
                                &mut st.decode,
                                "decoded, every consecutive step zero (checks below skip it)",
                            );
                        }
                        if let (Some(a), Some(b2), Some(c2)) =
                            (le, roughness(&be, ch), roughness(&offset, ch))
                        {
                            check(
                                &mut st.checks,
                                "PCM: little-endian signed reading has smaller steps between consecutive samples than big-endian",
                                a < b2,
                            );
                            check(
                                &mut st.checks,
                                "PCM: signed reading has smaller steps between consecutive samples than offset-binary",
                                a < c2,
                            );
                        }
                    }
                    st.frames += p.frames() as u64;
                    let secs = p.frames() as f64 / f64::from(p.sample_rate.max(1));
                    st.pcm_seconds += secs;
                    st.pcm_bytes += bytes.len() as u64;
                    st.pcm_files += 1;
                    bump(&mut st.durations, seconds_class(secs));
                }
                Err(e) => bump(&mut st.decode, format!("refused: {}", e.code)),
            }
        }
        Status::WmaOpen => {
            bump(&mut st.decode, "WMA family: not decoded, payload raw");
            bump(
                &mut st.avg,
                format!("average bytes per second {}", f.avg_bytes_per_sec),
            );
            let p = packets(w);
            check(
                &mut st.checks,
                "XWMA: data length a multiple of block align",
                p.is_some(),
            );
            if p.is_none() {
                let a = u64::from(f.block_align.max(1));
                bump(
                    &mut st.xwma,
                    format!(
                        "data length mod block align: {} (data length {})",
                        pow2(w.data.length % a),
                        if w.data.length % 2 == 1 {
                            "odd"
                        } else {
                            "even"
                        }
                    ),
                );
            }
            let full = w.data.length / u64::from(f.block_align.max(1));
            st.packets += full;
            let n = w.dpds.len() as u64;
            check(
                &mut st.checks,
                "XWMA: dpds entries == whole packets (data / block align)",
                n == full,
            );
            if n != full {
                bump(
                    &mut st.xwma,
                    format!("dpds entries - whole packets: {}", n as i64 - full as i64),
                );
            }
            let drop = w.dpds.windows(2).position(|x| x[0] >= x[1]);
            check(
                &mut st.checks,
                "XWMA: dpds strictly increasing",
                drop.is_none(),
            );
            if let Some(i) = drop {
                bump(
                    &mut st.xwma,
                    format!(
                        "first dpds entry not above the one before: {}, {}",
                        if i + 2 == w.dpds.len() {
                            "the last entry"
                        } else {
                            "an earlier entry"
                        },
                        if w.dpds[i + 1] < w.dpds[i] {
                            "below it"
                        } else {
                            "equal"
                        }
                    ),
                );
            }
            let mut steps: Vec<u32> = w.dpds.windows(2).map(|x| x[1].wrapping_sub(x[0])).collect();
            if let Some(first) = w.dpds.first() {
                steps.insert(0, *first);
            }
            let distinct: HashSet<u32> = steps.iter().copied().collect();
            check(
                &mut st.checks,
                "XWMA: every dpds step (first value included) equal within the file",
                distinct.len() <= 1,
            );
            check(
                &mut st.checks,
                "XWMA: every dpds step but the last equal within the file",
                steps.len() < 2
                    || steps[..steps.len() - 1]
                        .iter()
                        .collect::<HashSet<_>>()
                        .len()
                        == 1,
            );
            if let Some(s0) = steps.first() {
                bump(
                    &mut st.dpds_steps,
                    format!("channels {} first step {}", f.channels, s0),
                );
            }
            if let Some(last) = w.dpds.last() {
                let frame = 2 * u64::from(f.channels.max(1));
                check(
                    &mut st.checks,
                    "XWMA: last dpds a multiple of 2 * channels (candidate: 16-bit decoded bytes)",
                    u64::from(*last) % frame == 0,
                );
                let secs = f64::from(*last) / frame as f64 / f64::from(f.sample_rate.max(1));
                st.dpds_seconds += secs;
                bump(&mut st.durations, seconds_class(secs));
                if f.avg_bytes_per_sec > 0 && secs > 0.0 {
                    let by_rate = w.data.length as f64 / f64::from(f.avg_bytes_per_sec);
                    let r = by_rate / secs;
                    bump(
                        &mut st.xwma,
                        format!(
                            "data / average bytes per second over dpds duration: {}",
                            match r {
                                x if x < 0.5 => "< 0.5",
                                x if x < 0.9 => "0.5-0.9",
                                x if x <= 1.1 => "0.9-1.1",
                                x if x <= 2.0 => "1.1-2",
                                _ => "> 2",
                            }
                        ),
                    );
                }
            }
        }
        Status::Unsupported => bump(&mut st.decode, "unsupported format"),
    }
    if let Some(z) = &s.fuz {
        bump(&mut st.fuz, format!("version {}", z.version));
        bump(
            &mut st.fuz,
            format!("audio half form {}", fourcc(&w.riff.form)),
        );
        st.lip.put(z.lip.length);
        bump(&mut st.lip_sizes, pow2(z.lip.length));
        let lip = z.lip.of(bytes).unwrap_or_default();
        if lip.len() >= 4 {
            bump(
                &mut st.fuz,
                format!(
                    "lip half first four bytes {:02x} {:02x} {:02x} {:02x}",
                    lip[0], lip[1], lip[2], lip[3]
                ),
            );
        } else {
            bump(&mut st.fuz, format!("lip half of {} bytes", lip.len()));
        }
    }
    (decoded, Some(status))
}

fn print_stats(out: &mut String, name: &str, st: &Stats) {
    section(out, &format!("container {name}"));
    let _ = writeln!(out, "files\t{}", st.files);
    let _ = writeln!(out, "bytes\t{}", st.bytes);
    let p = &st.parts;
    let _ = writeln!(
        out,
        "bytes by part: voice header {} | lip half {} | RIFF framing {} | fmt {} | dpds {} | data {} | other chunks {} | total {}",
        p.fuz_header,
        p.lip,
        p.framing,
        p.format,
        p.seek,
        p.data,
        p.other,
        p.total()
    );
    counts(out, &format!("{name}: outcomes"), &st.outcomes);
    counts(out, &format!("{name}: entries by mount group"), &st.groups);
    counts(out, &format!("{name}: fmt fields"), &st.formats);
    if !st.avg.is_empty() {
        counts(
            out,
            &format!("{name}: fmt average bytes per second"),
            &st.avg,
        );
    }
    section(
        out,
        &format!("{name}: chunks (files with, occurrences, payload bytes, size range)"),
    );
    for (id, n) in &st.chunk_files {
        let _ = writeln!(
            out,
            "{id}\t{n}\t{}\t{}\t{}",
            st.chunk_occ[id], st.chunk_bytes[id], st.chunk_sizes[id]
        );
    }
    counts(out, &format!("{name}: chunk order"), &st.orders);
    if !st.pads.is_empty() {
        counts(out, &format!("{name}: pad bytes"), &st.pads);
    }
    for (t, m) in [("smpl", &st.smpl), ("cue", &st.cue), ("LIST", &st.list)] {
        if !m.is_empty() {
            counts(
                out,
                &format!("{name}: {t} chunk under its candidate layout"),
                m,
            );
        }
    }
    checks(out, &format!("{name}: checks"), &st.checks);
    counts(out, &format!("{name}: decoding"), &st.decode);
    if st.pcm_files > 0 {
        let _ = writeln!(
            out,
            "decoded PCM: files {} bytes {} frames {} seconds {:.0}",
            st.pcm_files, st.pcm_bytes, st.frames, st.pcm_seconds
        );
    }
    if st.packets > 0 {
        let _ = writeln!(
            out,
            "WMA packets (whole, data / block align)\t{}\nseconds under the dpds candidate\t{:.0}",
            st.packets, st.dpds_seconds
        );
        counts(out, &format!("{name}: XWMA framing"), &st.xwma);
        counts(out, &format!("{name}: dpds first step"), &st.dpds_steps);
    }
    counts(out, &format!("{name}: durations"), &st.durations);
    if !st.fuz.is_empty() {
        counts(out, &format!("{name}: voice container"), &st.fuz);
        let _ = writeln!(out, "lip half bytes\t{}", st.lip);
        counts(out, &format!("{name}: lip half sizes"), &st.lip_sizes);
    }
}

/// Loose files under the data root, by extension, and how many are sound
/// containers by their first bytes. Only listed and opened for reading.
fn loose(root: &Path, out: &mut String) -> std::io::Result<()> {
    let mut by_ext = Counts::new();
    let mut sound = Counts::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let p = e.path();
            let t = e.file_type()?;
            if t.is_dir() {
                stack.push(p);
                continue;
            }
            if !t.is_file() {
                continue;
            }
            let x = p
                .extension()
                .map(|x| x.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_else(|| "(none)".into());
            if matches!(x.as_str(), "ba2" | "esm" | "esp" | "esl") {
                continue;
            }
            bump(&mut by_ext, x.clone());
            let mut head = [0u8; 12];
            let n = {
                use std::io::Read;
                let mut f = std::fs::File::open(&p)?;
                let mut n = 0;
                while n < 12 {
                    let k = f.read(&mut head[n..])?;
                    if k == 0 {
                        break;
                    }
                    n += k;
                }
                n
            };
            if let Some(c) = Container::sniff(&head[..n]) {
                bump(&mut sound, format!("{} ({x})", c.label()));
            }
        }
    }
    counts(
        out,
        "loose files under the data root, by extension (archives and plugins left out)",
        &by_ext,
    );
    let _ = writeln!(
        out,
        "loose sound containers by first bytes\t{}",
        sound.values().sum::<u64>()
    );
    for (k, v) in &sound {
        let _ = writeln!(out, "{v}\t{k}");
    }
    Ok(())
}

pub fn run(
    m: &Manifest,
    vfs: &Vfs,
    out: &mut String,
    names: &mut String,
) -> Result<Index, Box<dyn std::error::Error>> {
    let c = &vfs.candidates;
    let mut keys = HashSet::new();
    let mut by_ext: BTreeMap<String, [u64; 4]> = BTreeMap::new();
    let mut magic: BTreeMap<String, Counts> = BTreeMap::new();
    let mut sound_entries: Vec<(usize, Container, bool)> = Vec::new();
    let mut not_sniffed = 0u64;
    let mut kinds = Counts::new();
    for a in &vfs.archives {
        add(&mut kinds, fourcc(&a.index.kind()), a.index.len() as u64);
    }
    for (i, x) in c.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        keys.insert(k.clone());
        let winner = c.get(i + 1).and_then(|n| n.key.as_ref()) != Some(k);
        let e = ext(k);
        let size = vfs.payload_size(x)?;
        let row = by_ext.entry(e.clone()).or_default();
        row[0] += 1;
        row[1] += size;
        let texture = matches!(&x.location, Location::Archive { archive, .. }
            if vfs.archives[*archive].index.kind() == *b"DX10");
        if texture {
            not_sniffed += 1;
            continue;
        }
        let prefix = vfs.read_prefix(x, 12, &Budget::new(1 << 20))?;
        if let Some(ct) = Container::sniff(&prefix.bytes) {
            row[2] += 1;
            row[3] += size;
            bump(magic.entry(ct.label().into()).or_default(), e);
            sound_entries.push((i, ct, winner));
        }
    }
    section(out, "archive entries");
    for (k, n) in &kinds {
        let _ = writeln!(out, "archive kind {k}: entries\t{n}");
    }
    let _ = writeln!(
        out,
        "entries\t{}\ndistinct paths\t{}\ntexture-archive entries (DDS by construction, first bytes not read)\t{}",
        c.len(),
        keys.len(),
        not_sniffed
    );
    section(
        out,
        "entries by extension: entries, bytes, entries whose first bytes name a sound container, their bytes",
    );
    for (e, [n, b, sn, sb]) in &by_ext {
        let _ = writeln!(out, "{e}\t{n}\t{b}\t{sn}\t{sb}");
    }
    section(out, "sound containers by first bytes, by extension");
    for (ct, m) in &magic {
        for (e, n) in m {
            let _ = writeln!(out, "{ct}\t{e}\t{n}");
        }
    }
    loose(Path::new(&m.root), out)?;

    let mut stats: BTreeMap<Container, Stats> = BTreeMap::new();
    let mut sounds = HashMap::new();
    let mut overridden = 0u64;
    for (i, ct, winner) in &sound_entries {
        let x = &c[*i];
        if !winner {
            overridden += 1;
        }
        let st = stats.entry(*ct).or_default();
        let b = Budget::new(m.limits.max_payload);
        let bytes = vfs.read_candidate(x, &b)?;
        st.files += 1;
        st.bytes += bytes.len() as u64;
        bump(&mut st.groups, group(vfs, &x.location));
        let (read, decoded, status) = match Sound::read(&bytes, &x.original, &b) {
            Ok(s) => {
                bump(&mut st.outcomes, "read to the last byte");
                let (d, status) = census_file(st, &s, &bytes, &b);
                (true, d, status)
            }
            Err(e) => {
                bump(&mut st.outcomes, format!("refused: {}", e.code));
                let _ = writeln!(names, "refused\t{}\t{}", x.original, e);
                (false, false, None)
            }
        };
        if *winner {
            if let Some(k) = &x.key {
                sounds.insert(
                    k.clone(),
                    Info {
                        container: *ct,
                        read,
                        status,
                        decoded,
                    },
                );
            }
        }
    }
    section(out, "sound files");
    let _ = writeln!(
        out,
        "sound entries (every candidate)\t{}\noverridden copies among them\t{}",
        sound_entries.len(),
        overridden
    );
    let mut total = Parts::default();
    let (mut files, mut bytes, mut read, mut decoded_files, mut decoded_bytes) = (0, 0, 0, 0, 0);
    for st in stats.values() {
        add_parts(&mut total, &st.parts);
        files += st.files;
        bytes += st.bytes;
        read += st
            .outcomes
            .get("read to the last byte")
            .copied()
            .unwrap_or(0);
        decoded_files += st.pcm_files;
        decoded_bytes += st.pcm_bytes;
    }
    let _ = writeln!(
        out,
        "files\t{files}\nbytes\t{bytes}\nfiles read to the last byte\t{read}\nbytes read to the last byte\t{}\nfiles decoded to PCM\t{decoded_files}\nbytes of the files decoded to PCM\t{decoded_bytes}",
        total.total()
    );
    for (ct, st) in &stats {
        print_stats(out, ct.label(), st);
    }
    Ok(Index { keys, sounds })
}
