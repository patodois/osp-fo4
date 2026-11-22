#![forbid(unsafe_code)]
//! Census of spoken lines and their voice files (`spec/voice.md`):
//! `voice_census MANIFEST OUT.txt NAMES.txt`.
//!
//! OUT.txt holds counts, record type tags, field values and fits only
//! (committed as `spec/voice-census.txt`); NAMES.txt names the voice type
//! folders, records and files behind the counts and is never committed.
//!
//! Part 1: every mounted voice entry (`sound/voice/...`) by depth,
//! extension and whether its path follows the convention
//! (`fo4_vm::voice::parse_path`); its plugin and voice type folders
//! against the loaded plugins and the VTYP editor ids. Part 2: every
//! winning INFO held by a topic: its responses (TRDA size, the response
//! number byte), its shared line (DNAM). Part 3: the convention against
//! the records: the files of every response, the responses of every
//! line, the files no response names, coverage per voice type folder.
//! Part 4: the scene actions: the topic of every dialogue action, its
//! lines, their files, the speaker alias's voice type against the folders
//! that hold the files; the stored order of the player dialogue topics;
//! the timer action fields. Part 5: the header of every voice file: format,
//! the seconds the last `dpds` word gives (`fo4_audio::header_seconds`), the
//! stored data bytes fitted against those seconds, the `dpds` steps.
use fo4_audio::Sound;
use fo4_core::{Budget, Manifest};
use fo4_plugin::{actor::Context, info::Info, scen::Scene, typed::Store, FormKey};
use fo4_vfs::Vfs;
use fo4_vm::{
    bind,
    voice::{self, Speaker},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
    path::Path,
};

type E = Box<dyn std::error::Error>;
type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}
fn holds(c: &mut Counts, what: &str, ok: bool) {
    bump(c, format!("{what}\t{}", if ok { "holds" } else { "fails" }));
}
fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        _ => ">=20",
    }
}
fn seconds_class(s: f64) -> &'static str {
    match s {
        x if x < 1.0 => "< 1 s",
        x if x < 2.0 => "1-2 s",
        x if x < 5.0 => "2-5 s",
        x if x < 10.0 => "5-10 s",
        x if x < 30.0 => "10-30 s",
        _ => ">= 30 s",
    }
}
fn share_class(hit: u64, all: u64) -> &'static str {
    if all == 0 {
        return "no files";
    }
    match hit * 100 / all {
        100 => "100%",
        90..=99 => "90-99%",
        50..=89 => "50-89%",
        1..=49 => "1-49%",
        _ => "0%",
    }
}

/// Least-squares line of y on x.
#[derive(Default)]
struct Fit {
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
}
impl Fit {
    fn put(&mut self, x: f64, y: f64) {
        self.n += 1.0;
        self.sx += x;
        self.sy += y;
        self.sxx += x * x;
        self.sxy += x * y;
    }
    fn line(&self) -> Option<(f64, f64)> {
        let d = self.n * self.sxx - self.sx * self.sx;
        if self.n < 2.0 || d == 0.0 {
            return None;
        }
        let slope = (self.n * self.sxy - self.sx * self.sy) / d;
        Some((slope, (self.sy - slope * self.sx) / self.n))
    }
}

/// One mounted voice file following the convention.
struct File {
    candidate: usize,
    path: voice::VoicePath,
}

fn run() -> Result<(), E> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 3 {
        return Err("voice_census MANIFEST OUT.txt NAMES.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let b = Budget::new(m.limits.max_memory);
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let vfs = Vfs::mount(&m, &Budget::new(m.limits.max_vfs_memory))?;
    let mut c = Counts::new();
    let mut names = String::new();
    let label = |k: &FormKey| -> String {
        let e = store
            .raw(k, &b.child())
            .ok()
            .flatten()
            .and_then(|r| bind::editor_id(&r))
            .unwrap_or_default();
        format!("{}:{:06x} {e}", k.plugin, k.object_id)
    };

    // Part 1: voice entries.
    let plugins: HashSet<String> = m
        .plugins
        .iter()
        .map(|p| p.name.to_ascii_lowercase())
        .collect();
    let mut vtyp: HashSet<String> = HashSet::new();
    for o in store.winners() {
        if o.signature != *b"VTYP" || o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        if let Some(e) = store.raw(&k, &b.child())?.and_then(|r| bind::editor_id(&r)) {
            vtyp.insert(e.to_ascii_lowercase());
        }
    }
    bump(&mut c, format!("1 winning VTYP records\t{}", vtyp.len()));
    let mut files: Vec<File> = Vec::new();
    let mut folders: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut plugin_folders: BTreeSet<String> = BTreeSet::new();
    for (i, x) in vfs.candidates.iter().enumerate() {
        let Some(k) = &x.key else { continue };
        let winner = vfs.candidates.get(i + 1).and_then(|n| n.key.as_ref()) != Some(k);
        if !winner || !k.starts_with(b"sound/voice/") {
            continue;
        }
        let p = String::from_utf8_lossy(k);
        let depth = p.split('/').count();
        let ext = p.rsplit_once('.').map(|x| x.1).unwrap_or("(none)");
        bump(
            &mut c,
            format!("1 voice entries by depth and extension\t{depth}\t{ext}"),
        );
        match voice::parse_path(k) {
            Some(v) => {
                bump(
                    &mut c,
                    format!("1 voice entries follow the convention\t{ext}"),
                );
                plugin_folders.insert(v.plugin.clone());
                folders.entry(v.voice_type.clone()).or_default().0 += 1;
                files.push(File {
                    candidate: i,
                    path: v,
                });
            }
            None => {
                let name = p.rsplit('/').next().unwrap_or("");
                let class = if depth != 5 {
                    "another depth"
                } else if !name.ends_with(".fuz") {
                    "another extension"
                } else if name.contains("_alt") {
                    "a .fuz name with a further suffix (_alt...)"
                } else {
                    "a .fuz name of another shape"
                };
                bump(
                    &mut c,
                    format!("1 voice entries outside the convention\t{class}"),
                );
                let _ = writeln!(names, "OUTSIDE {class}\t{p}");
            }
        }
    }
    for p in &plugin_folders {
        holds(
            &mut c,
            "1 plugin folder is a loaded plugin's name",
            plugins.contains(p),
        );
    }
    for v in folders.keys() {
        holds(
            &mut c,
            "1 voice type folder is a VTYP editor id",
            vtyp.contains(v),
        );
        if !vtyp.contains(v) {
            let _ = writeln!(names, "FOLDER NOT A VTYP\t{v}");
        }
    }
    bump(&mut c, format!("1 voice type folders\t{}", folders.len()));
    let mut by_response: HashMap<(String, u32, u8), Vec<usize>> = HashMap::new();
    for (j, f) in files.iter().enumerate() {
        by_response
            .entry((f.path.plugin.clone(), f.path.object_id, f.path.response))
            .or_default()
            .push(j);
    }

    // Part 2 and 3: lines, responses and their files.
    let topics = voice::Topics::read(&store);
    let mut matched = vec![false; files.len()];
    let mut infos: HashMap<String, (Vec<u8>, Option<FormKey>)> = HashMap::new();
    for (o, _) in store.topic_children() {
        if o.signature != *b"INFO" || o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let Some((r, plugin)) = store.raw_with_plugin(&k, &b.child())? else {
            continue;
        };
        for s in &r.subrecords {
            if s.signature == *b"TRDA" {
                bump(&mut c, format!("2 TRDA size\t{}", r.data(s).len()));
            }
        }
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let info = match Info::read(&r, ctx, &b.child()) {
            Ok(i) => i,
            Err(e) => {
                bump(&mut c, format!("2 INFO refused\t{}", e.code));
                continue;
            }
        };
        bump(&mut c, "2 winning INFO held by a topic");
        bump(
            &mut c,
            format!("2 responses per INFO\t{}", bucket(info.responses.len())),
        );
        let nums: Vec<u8> = info.responses.iter().map(|x| x.number).collect();
        let distinct: BTreeSet<u8> = nums.iter().copied().collect();
        if !nums.is_empty() {
            holds(
                &mut c,
                "2 response numbers distinct within an INFO",
                distinct.len() == nums.len(),
            );
            holds(
                &mut c,
                "2 response numbers increasing in stored order",
                nums.windows(2).all(|w| w[0] < w[1]),
            );
            holds(
                &mut c,
                "2 response numbers are 1 to the count",
                distinct.iter().copied().eq(1..=nums.len() as u8),
            );
        }
        for x in &info.responses {
            holds(
                &mut c,
                "2 TRDA bytes 5 to 7 zero",
                x.trda[5..8] == [0, 0, 0],
            );
        }
        if let Some(s) = &info.shared {
            let t = s.key.as_ref().and_then(|k| store.signature(k));
            bump(
                &mut c,
                format!(
                    "2 DNAM names\t{}\tINFO has {} responses",
                    t.map_or("no winning record".into(), |t| String::from_utf8_lossy(&t)
                        .into_owned()),
                    if info.responses.is_empty() {
                        "no"
                    } else {
                        "some"
                    }
                ),
            );
        }
        // Files of each response, in the INFO's own plugin folder.
        let mut with = 0;
        let mut vts: BTreeSet<String> = BTreeSet::new();
        for x in &info.responses {
            let key = (k.plugin.clone(), k.object_id, x.number);
            match by_response.get(&key) {
                Some(v) => {
                    with += 1;
                    bump(
                        &mut c,
                        format!(
                            "3 voice type folders per response with a file\t{}",
                            bucket(v.len())
                        ),
                    );
                    for &j in v {
                        matched[j] = true;
                        vts.insert(files[j].path.voice_type.clone());
                    }
                }
                None => {
                    let other = plugin_folders.iter().any(|p| {
                        *p != k.plugin
                            && by_response.contains_key(&(p.clone(), k.object_id, x.number))
                    });
                    bump(&mut c, format!("3 responses without a file in the line's plugin folder\tsame name in another plugin folder: {other}"));
                }
            }
        }
        let cls = match (info.responses.len(), with) {
            (0, _) => "no responses",
            (n, w) if w == n => "every response has a file",
            (_, 0) => "no response has a file",
            _ => "some responses have a file",
        };
        bump(&mut c, format!("3 INFO voice coverage\t{cls}"));
        if !info.responses.is_empty() {
            bump(
                &mut c,
                format!(
                    "3 voice type folders per INFO with a file\t{}",
                    bucket(vts.len())
                ),
            );
        }
        infos.insert(
            bind::record_key(&k),
            (nums, info.shared.as_ref().and_then(|s| s.key.clone())),
        );
    }
    // Shared lines: their target's responses and files.
    for (nums, shared) in infos.values() {
        let Some(s) = shared else { continue };
        if !nums.is_empty() {
            continue;
        }
        let target = infos.get(&bind::record_key(s));
        let cls = match target {
            None => "names no INFO held by a topic",
            Some((t, _)) if t.is_empty() => "names an INFO without responses",
            Some((t, _)) => {
                if t.iter()
                    .all(|n| by_response.contains_key(&(s.plugin.clone(), s.object_id, *n)))
                {
                    "names an INFO whose every response has a file"
                } else {
                    "names an INFO with a response without a file"
                }
            }
        };
        bump(
            &mut c,
            format!("3 INFO without responses whose DNAM\t{cls}"),
        );
    }
    let mut unmatched_shape = Counts::new();
    for (j, f) in files.iter().enumerate() {
        holds(
            &mut c,
            "3 voice file named by a response of its line",
            matched[j],
        );
        if matched[j] {
            folders.entry(f.path.voice_type.clone()).or_default().1 += 1;
        } else {
            let k = FormKey {
                plugin: f.path.plugin.clone(),
                object_id: f.path.object_id,
            };
            let what = match store.signature(&k) {
                Some(s) if s == *b"INFO" => {
                    if store.overrides().winner(&k).is_some_and(|w| w.deleted) {
                        "a deleted INFO".to_string()
                    } else {
                        "an INFO without that response number".to_string()
                    }
                }
                Some(s) => format!("a {} record", String::from_utf8_lossy(&s)),
                None => "no winning record".to_string(),
            };
            bump(
                &mut unmatched_shape,
                format!("3 voice file named by no response: its id names\t{what}"),
            );
        }
    }
    c.extend(unmatched_shape);
    for (v, (all, hit)) in &folders {
        bump(
            &mut c,
            format!(
                "3 voice type folders by share of files a response names\t{}",
                share_class(*hit, *all)
            ),
        );
        let _ = writeln!(names, "FOLDER\t{v}\tfiles {all}\tnamed by a response {hit}");
    }

    // Part 4: scene actions.
    let files_index = {
        let mut f = voice::VoiceFiles::default();
        for x in &files {
            let key = &vfs.candidates[x.candidate].key;
            if let Some(k) = key {
                f.add(k, Some(0.0));
            }
        }
        f
    };
    let mut empty_topics = BTreeSet::new();
    for o in store.winners() {
        if o.signature != *b"SCEN" || o.deleted {
            continue;
        }
        let k = store.overrides().form_key(o.key, store.order());
        let Some((r, plugin)) = store.raw_with_plugin(&k, &b.child())? else {
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        let s = Scene::read(&r, ctx, &b.child())?;
        for act in &s.actions {
            match act.kind {
                0 => {
                    bump(&mut c, "4 dialogue actions (kind 0)");
                    let Some(t) = act
                        .field(b"DATA")
                        .and_then(|f| f.form.as_ref())
                        .and_then(|f| f.key.clone())
                    else {
                        bump(&mut c, "4 dialogue action topic\tDATA names no record");
                        continue;
                    };
                    let sig = store
                        .signature(&t)
                        .map(|s| String::from_utf8_lossy(&s).into_owned());
                    bump(
                        &mut c,
                        format!(
                            "4 dialogue action topic\tDATA names {}",
                            sig.as_deref().unwrap_or("no winning record")
                        ),
                    );
                    let lines = topics.of(&t);
                    bump(
                        &mut c,
                        format!("4 lines in the action's topic\t{}", bucket(lines.len())),
                    );
                    if lines.is_empty() {
                        if let Some(r) = store.raw(&t, &b.child())? {
                            let tags: Vec<String> = r
                                .subrecords
                                .iter()
                                .map(|s| String::from_utf8_lossy(&s.signature).into_owned())
                                .collect();
                            let tifc = r
                                .subrecords
                                .iter()
                                .find(|s| s.signature == *b"TIFC")
                                .map(|s| r.data(s).to_vec());
                            let tifc = tifc.and_then(|d| d.try_into().ok().map(u32::from_le_bytes));
                            bump(&mut c, format!("4 topic without lines: its TIFC\t{tifc:?}"));
                            let _ = tags;
                        }
                        empty_topics.insert(label(&t));
                        continue;
                    }
                    // Responses with files, over every line of the topic.
                    let mut all = true;
                    let mut vts: BTreeSet<String> = BTreeSet::new();
                    for l in lines {
                        let Some((nums, _)) = infos.get(&bind::record_key(l)) else {
                            continue;
                        };
                        for n in nums {
                            let f = files_index.get(l, *n);
                            all &= !f.is_empty();
                            vts.extend(f.iter().map(|x| x.voice_type.clone()));
                        }
                    }
                    holds(
                        &mut c,
                        "4 every response of every line of the topic has a file",
                        all,
                    );
                    bump(
                        &mut c,
                        format!(
                            "4 voice type folders over the topic's lines\t{}",
                            bucket(vts.len())
                        ),
                    );
                    // The speaker's voice type against the folders.
                    let sp = match &s.quest.key {
                        Some(q) => voice::speaker(&store, q, act.alias, &b.child()),
                        None => Speaker::Unknown("the scene has no quest".into()),
                    };
                    match sp {
                        Speaker::VoiceType(v, how) => {
                            bump(&mut c, format!("4 speaker voice type settled\t{how}"));
                            if !vts.is_empty() {
                                holds(&mut c, &format!("4 the settled voice type's folder holds the topic's files\t{how}"), vts.contains(&v));
                                if !vts.contains(&v) {
                                    let _ = writeln!(
                                        names,
                                        "SPEAKER FOLDER MISSING\t{}\t{v}\t{vts:?}",
                                        label(&k)
                                    );
                                }
                            }
                        }
                        Speaker::Unknown(why) => {
                            bump(&mut c, format!("4 speaker voice type not settled\t{why}"))
                        }
                    }
                }
                2 => {
                    bump(&mut c, "4 timer actions (kind 2)");
                    let tags: Vec<String> = act
                        .fields
                        .iter()
                        .map(|f| String::from_utf8_lossy(&f.tag).into_owned())
                        .collect();
                    bump(
                        &mut c,
                        format!(
                            "4 timer action fields after the end phase\t{}\tFNAM {:#010x}",
                            tags.join(" "),
                            act.flags.unwrap_or(0)
                        ),
                    );
                    if let Some(x) = act.float(b"SNAM") {
                        bump(&mut c, format!("4 timer SNAM as f32\t{x}"));
                        if let Some(y) = act.float(b"TNAM") {
                            bump(
                                &mut c,
                                format!(
                                    "4 timer TNAM as f32 against SNAM\t{}",
                                    if y < x {
                                        "lower"
                                    } else if y == x {
                                        "equal"
                                    } else {
                                        "higher"
                                    }
                                ),
                            );
                            bump(&mut c, format!("4 timer TNAM as f32\t{y}"));
                        }
                    }
                }
                3 => {
                    bump(&mut c, "4 player dialogue actions (kind 3)");
                    let order: Vec<String> = act
                        .fields
                        .iter()
                        .filter(|f| {
                            fo4_vm::scene::PLAYER_CHOICES
                                .iter()
                                .any(|(p, r)| f.tag == *p || f.tag == *r)
                        })
                        .map(|f| String::from_utf8_lossy(&f.tag).into_owned())
                        .collect();
                    bump(
                        &mut c,
                        format!(
                            "4 player dialogue topic fields in stored order\t{}",
                            order.join(" ")
                        ),
                    );
                }
                _ => {}
            }
        }
    }
    for t in &empty_topics {
        let _ = writeln!(names, "TOPIC WITHOUT LINES\t{t}");
    }

    // Part 5: voice file headers.
    let mut fits: BTreeMap<String, Fit> = BTreeMap::new();
    let mut rows: Vec<(String, f64, f64)> = Vec::new();
    for f in &files {
        let x = &vfs.candidates[f.candidate];
        let bb = Budget::new(m.limits.max_payload);
        let bytes = vfs.read_candidate(x, &bb)?;
        let s = match Sound::read(&bytes, &x.original, &bb) {
            Ok(s) => s,
            Err(e) => {
                bump(&mut c, format!("5 voice file refused\t{}", e.code));
                continue;
            }
        };
        let w = &s.wave.format;
        let kind = format!(
            "{} tag {:#x} channels {} rate {} average bytes {} block align {}",
            s.container.label(),
            w.tag,
            w.channels,
            w.sample_rate,
            w.avg_bytes_per_sec,
            w.block_align
        );
        bump(&mut c, format!("5 voice file format\t{kind}"));
        let Some(secs) = fo4_audio::header_seconds(&s.wave) else {
            bump(&mut c, "5 header declares no seconds");
            continue;
        };
        bump(
            &mut c,
            format!("5 seconds from the header\t{}", seconds_class(secs)),
        );
        fits.entry(kind.clone())
            .or_default()
            .put(secs, s.wave.data.length as f64);
        rows.push((kind, secs, s.wave.data.length as f64));
        let frame = 2 * u32::from(w.channels);
        let mut prev = 0u32;
        let n = s.wave.dpds.len();
        for (j, d) in s.wave.dpds.iter().enumerate() {
            let step = d.wrapping_sub(prev);
            prev = *d;
            let what = if step % frame != 0 {
                "not whole frames"
            } else {
                let fr = step / frame;
                if fr == 0 {
                    "zero"
                } else if fr % 2048 == 0 {
                    "a multiple of 2,048 frames"
                } else if fr % 512 == 0 {
                    "a multiple of 512 frames, not of 2,048"
                } else if fr % 128 == 0 {
                    "a multiple of 128 frames, not of 512"
                } else {
                    "whole frames, not a multiple of 128"
                }
            };
            bump(
                &mut c,
                format!(
                    "5 dpds step\t{}\t{what}",
                    if j + 1 == n {
                        "last packet"
                    } else {
                        "other packets"
                    }
                ),
            );
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# voice_census: counts, record type tags, field values and fits only (spec/voice.md)"
    );
    let _ = writeln!(out, "# 1 voice entries; 2 lines (INFO) and responses; 3 the path convention against the records; 4 scene actions; 5 voice file headers");
    for (k, v) in &c {
        let _ = writeln!(out, "{v}\t{k}");
    }
    for (k, f) in &fits {
        let Some((slope, icpt)) = f.line() else {
            continue;
        };
        let _ = writeln!(out, "5 fit of stored data bytes on header seconds\t{k}\tfiles {}\tslope {slope:.1} bytes per second\tintercept {icpt:.1} bytes", f.n);
        let mut r = Counts::new();
        for (kk, x, y) in rows.iter().filter(|r| &r.0 == k) {
            let _ = kk;
            let p = slope * x + icpt;
            let d = ((y - p) / p).abs();
            let cls = match d {
                d if d < 0.05 => "within 5%",
                d if d < 0.10 => "5-10%",
                d if d < 0.25 => "10-25%",
                _ => "25% or more",
            };
            bump(&mut r, cls);
        }
        for (cls, n) in r {
            let _ = writeln!(out, "5 data bytes against the fit\t{k}\t{cls}\t{n}");
        }
    }
    std::fs::write(&a[1], out)?;
    std::fs::write(&a[2], names)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
