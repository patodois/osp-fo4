#![forbid(unsafe_code)]
//! Census of every winning QUST record read with `fo4_plugin::quest`
//! (`spec/quest.md`): records read, bytes typed and raw by subrecord tag,
//! the subrecord order observed, flag and value histograms, stages,
//! log entries, objectives, aliases and their fill subrecords, alias
//! index fields checked against the quest's aliases, FormID fields by the
//! record type they name, and how each stage fragment of the VMAD block
//! maps to a stage and log entry of the record. Measurement support;
//! `quest_census MANIFEST OUT.txt` writes counts only (tags, record types,
//! numbers; no name, FormID or text).
use fo4_core::{Budget, Manifest};
use fo4_plugin::{
    actor::Context,
    quest::{AliasKind, AliasValue, Quest},
    typed::{FormRef, Store},
    vmad::Vmad,
    FormKey,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write as _,
    path::Path,
};

type Counts = BTreeMap<String, u64>;
fn bump(m: &mut Counts, k: impl Into<String>) {
    add(m, k, 1);
}
fn add(m: &mut Counts, k: impl Into<String>, n: u64) {
    *m.entry(k.into()).or_default() += n;
}
fn tag(t: &[u8; 4]) -> String {
    String::from_utf8_lossy(t).into_owned()
}
fn bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3..=4 => "3-4",
        5..=9 => "5-9",
        10..=19 => "10-19",
        20..=49 => "20-49",
        _ => ">=50",
    }
}
fn bits(m: &mut Counts, what: &str, v: u32, width: u32) {
    for b in 0..width {
        if v & (1 << b) != 0 {
            bump(m, format!("{what} bit {b} (0x{:x}) set", 1u32 << b));
        }
    }
    if v == 0 {
        bump(m, format!("{what} zero"));
    }
}

struct Ctx<'a> {
    store: &'a Store,
    /// Alias ids by quest: (id, kind) with kind "reference", "location".
    aliases: &'a HashMap<FormKey, Vec<(u32, &'static str)>>,
    c: Counts,
}
impl Ctx<'_> {
    /// The record type a FormID field names, by tag.
    fn form(&mut self, what: &str, f: &FormRef) {
        let k = match &f.key {
            None => "zero".to_string(),
            Some(k) => match self.store.signature(k) {
                Some(s) => format!("a winning {}", tag(&s)),
                None => "no winning record".into(),
            },
        };
        bump(&mut self.c, format!("form field {what} names {k}"));
    }
    /// Whether an alias index names an alias of `quest`.
    fn alias(&mut self, what: &str, quest: Option<&FormKey>, id: i64) {
        let r = match quest.and_then(|q| self.aliases.get(q)) {
            None => "the quest has no alias list".to_string(),
            Some(list) => match list.iter().find(|x| i64::from(x.0) == id) {
                Some((_, kind)) => format!("names a {kind} alias of the quest"),
                None if id < 0 => format!("negative ({id})"),
                None => "names no alias of the quest".into(),
            },
        };
        bump(&mut self.c, format!("alias index {what} {r}"));
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 2 {
        return Err("quest_census MANIFEST OUT.txt".into());
    }
    let m = Manifest::load(Path::new(&a[0]))?;
    let store = Store::open(&m, &Budget::new(m.limits.max_override_memory))?;
    let winners: Vec<(FormKey, usize)> = store
        .winners()
        .filter(|w| !w.deleted && w.signature == *b"QUST")
        .map(|w| (store.overrides().form_key(w.key, store.order()), w.plugin))
        .collect();
    let mut errors = Counts::new();
    let mut quests: Vec<(FormKey, Quest, Option<fo4_plugin::Record>)> = Vec::new();
    let mut aliases: HashMap<FormKey, Vec<(u32, &'static str)>> = HashMap::new();
    for (k, _) in &winners {
        let b = Budget::new(m.limits.max_memory);
        let Some((r, plugin)) = store.raw_with_plugin(k, &b)? else {
            bump(&mut errors, "record not read");
            continue;
        };
        let ctx = Context {
            order: store.order(),
            plugin,
        };
        match Quest::read(&r, ctx, &b) {
            Ok(q) => {
                aliases.insert(
                    k.clone(),
                    q.aliases
                        .iter()
                        .map(|x| {
                            let kind = if x.kind == AliasKind::Location {
                                "location"
                            } else {
                                "reference"
                            };
                            (x.id, kind)
                        })
                        .collect(),
                );
                quests.push((k.clone(), q, Some(r)));
            }
            Err(e) => bump(&mut errors, format!("read error {}", e.code)),
        }
    }
    let mut cx = Ctx {
        store: &store,
        aliases: &aliases,
        c: Counts::new(),
    };
    let mut bytes = Counts::new();
    let mut trans = Counts::new();
    let mut frag = Counts::new();
    let mut fill = Counts::new();
    for (key, q, r) in &quests {
        let r = r.as_ref().ok_or("record")?;
        let c = &mut cx.c;
        // Bytes typed and raw by tag; VMAD as read by fo4_plugin::vmad.
        let mut prev = "^".to_string();
        let mut vmad = None;
        for f in &q.fields {
            let s = &r.subrecords[f.index];
            let mut typed = f.typed;
            if f.tag == *b"VMAD" {
                let b = Budget::new(m.limits.max_memory);
                match Vmad::read(r.data(s), r.signature, &r.source.file, s.offset, &b) {
                    Ok(v) if v.tail.is_empty() => {
                        typed = true;
                        vmad = Some(v);
                    }
                    _ => {}
                }
            }
            let kind = if typed { "typed" } else { "raw" };
            add(
                &mut bytes,
                format!("{}\t{kind}\tbytes", tag(&f.tag)),
                f.size as u64,
            );
            add(
                &mut bytes,
                format!("{}\t{kind}\tsubrecords", tag(&f.tag)),
                1,
            );
            add(&mut bytes, format!("all\t{kind}\tbytes",), f.size as u64);
            let t = tag(&f.tag);
            bump(&mut trans, format!("{prev}\t{t}"));
            prev = t;
        }
        bump(&mut trans, format!("{prev}\t$"));
        // General data.
        match &q.general {
            Some(g) => {
                bits(c, "DNAM flags", u32::from(g.flags), 16);
                bump(c, format!("DNAM priority {}", bucket(g.priority as usize)));
                bump(c, format!("DNAM byte 3 = {}", g.byte3));
                bump(c, format!("DNAM word at 4 = {}", g.word));
                bump(c, format!("DNAM u32 at 8 = {}", g.kind));
            }
            None => bump(c, "quests without DNAM"),
        }
        if q.event.is_some() {
            bump(c, "quests with ENAM");
        }
        bump(
            c,
            format!(
                "quests with {} conditions before NEXT",
                bucket(q.conditions.len())
            ),
        );
        bump(
            c,
            format!(
                "quests with {} conditions after NEXT",
                bucket(q.event_conditions.len())
            ),
        );
        // Stages.
        bump(c, format!("quests with {} stages", bucket(q.stages.len())));
        let mut startup = 0;
        let mut seen = BTreeSet::new();
        for s in &q.stages {
            if !seen.insert(s.stage) {
                bump(
                    c,
                    "stages whose index repeats an earlier stage of the quest",
                );
            }
            bump(c, format!("INDX flags byte = 0x{:02x}", s.flags));
            if s.flags & 2 != 0 {
                startup += 1;
            }
            bump(
                c,
                format!("stages with {} log entries", bucket(s.entries.len())),
            );
            for (i, e) in s.entries.iter().enumerate() {
                bump(c, format!("QSDT flags byte = 0x{:02x}", e.flags));
                bump(
                    c,
                    format!("log entries with {} conditions", bucket(e.conditions.len())),
                );
                if !e.conditions.is_empty() && i + 1 < s.entries.len() {
                    bump(c, "log entries with conditions followed by another entry");
                }
                bump(c, format!("log entries with NAM2 {}", e.note.is_some()));
                bump(c, format!("log entries with CNAM {}", e.text.is_some()));
            }
        }
        let start_game = q.general.is_some_and(|g| g.flags & 1 != 0);
        bump(
            c,
            format!(
                "quests with DNAM flag bit 0 {} and {} stages with INDX flag 0x02",
                start_game,
                bucket(startup)
            ),
        );
        // Objectives.
        bump(
            c,
            format!("quests with {} objectives", bucket(q.objectives.len())),
        );
        for o in &q.objectives {
            bits(c, "QOBJ FNAM", o.flags, 32);
            bump(
                c,
                format!("objectives with {} targets", bucket(o.targets.len())),
            );
            for t in &o.targets {
                bits(c, "QSTA flags", t.flags, 32);
            }
        }
        let objs: Vec<_> = q.objectives.iter().map(|o| o.objective).collect();
        if objs.iter().collect::<BTreeSet<_>>().len() != objs.len() {
            bump(c, "quests with a repeated objective index");
        }
        // Aliases.
        bump(
            c,
            format!("quests with {} aliases", bucket(q.aliases.len())),
        );
        if let (Some(n), Some(max)) = (q.next_alias, q.aliases.iter().map(|a| a.id).max()) {
            bump(c, format!("ANAM above every alias id {}", n > max));
        }
        let ids: Vec<_> = q.aliases.iter().map(|a| a.id).collect();
        if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
            bump(c, "quests with a repeated alias id");
        }
        let targets: Vec<(i32, bool)> = q
            .objectives
            .iter()
            .flat_map(|o| o.targets.iter().map(|t| (t.alias, t.keyword.key.is_some())))
            .collect();
        let _ = c;
        for (alias, _) in &targets {
            cx.alias("QSTA at 0", Some(key), i64::from(*alias));
        }
        for t in q.objectives.iter().flat_map(|o| &o.targets) {
            cx.form("QSTA at 8", &t.keyword);
        }
        for f in [&q.location, &q.xnam, &q.gnam].into_iter().flatten() {
            let t = tag(&r.subrecords[f.subrecord].signature);
            cx.form(&t, f);
        }
        for f in &q.text_globals {
            cx.form("QTGL", f);
        }
        for al in &q.aliases {
            let kind = match al.kind {
                AliasKind::Reference => "reference".to_string(),
                AliasKind::Location => "location".to_string(),
                AliasKind::Collection { max_initial, .. } => {
                    bump(
                        &mut cx.c,
                        format!("ALMI = {}", bucket(max_initial as usize)),
                    );
                    "collection".to_string()
                }
            };
            bump(&mut cx.c, format!("aliases {kind}"));
            bits(&mut cx.c, &format!("alias FNAM ({kind})"), al.flags, 32);
            let mut kinds = BTreeSet::new();
            let mut conditions = 0;
            for v in &al.values {
                let t = tag(&v.tag());
                bump(&mut cx.c, format!("alias values {t}"));
                match v {
                    AliasValue::ForcedReference(f) => {
                        kinds.insert("ALFR");
                        cx.form("ALFR", f);
                        let placed = f.key.as_ref().and_then(|k| {
                            store
                                .overrides()
                                .winner(k)
                                .map(|w| (w.signature, w.deleted))
                        });
                        let what = match placed {
                            Some((s, false)) if matches!(&s, b"REFR" | b"ACHR") => {
                                format!("a placed {} (winning, not deleted)", tag(&s))
                            }
                            Some((s, true)) => format!("a deleted {}", tag(&s)),
                            Some((s, _)) => format!("a {}", tag(&s)),
                            None => "no winning record".into(),
                        };
                        bump(&mut fill, format!("ALFR names {what}, alias kind {kind}"));
                    }
                    AliasValue::UniqueActor(f) => {
                        kinds.insert("ALUA");
                        cx.form("ALUA", f);
                    }
                    AliasValue::ForcedLocation(f) => {
                        kinds.insert("ALFL");
                        cx.form("ALFL", f);
                    }
                    AliasValue::LocationAliasReference { alias, ref_type } => {
                        kinds.insert("ALFA");
                        cx.alias("ALFA", Some(key), i64::from(*alias));
                        if let Some(f) = ref_type {
                            cx.form("ALRT", f);
                        } else {
                            bump(&mut cx.c, "ALFA without ALRT");
                        }
                    }
                    AliasValue::FromEvent { .. } => {
                        kinds.insert("ALFE");
                    }
                    AliasValue::CreateObject {
                        object,
                        alias,
                        word,
                        level,
                    } => {
                        kinds.insert("ALCO");
                        cx.form("ALCO", object);
                        cx.alias("ALCA low half", Some(key), i64::from(*alias));
                        bump(&mut cx.c, format!("ALCA high half = 0x{word:04x}"));
                        bump(&mut cx.c, format!("ALCL = {level}"));
                    }
                    AliasValue::External { quest, alias } => {
                        kinds.insert("ALEQ");
                        cx.form("ALEQ", quest);
                        cx.alias(
                            "ALEA (in the ALEQ quest)",
                            quest.key.as_ref(),
                            i64::from(*alias),
                        );
                    }
                    AliasValue::Alfi(alias) => {
                        kinds.insert("ALFI");
                        cx.alias("ALFI", Some(key), i64::from(*alias));
                    }
                    AliasValue::LinkedAlias { alias, word } => {
                        kinds.insert("ALNA");
                        cx.alias("ALNA", Some(key), i64::from(*alias));
                        bump(&mut cx.c, format!("ALNT = {word}"));
                    }
                    AliasValue::Alcc(v) => {
                        bump(&mut cx.c, format!("ALCC = {}", bucket(*v as usize)));
                    }
                    AliasValue::Form(t, f) => cx.form(&tag(t), f),
                    AliasValue::Keywords(v) => {
                        for f in v {
                            cx.form("KWDA", f);
                        }
                    }
                    AliasValue::Items(v) => {
                        for (f, n) in v {
                            cx.form("CNTO at 0", f);
                            bump(&mut cx.c, format!("CNTO count {}", bucket(*n as usize)));
                        }
                    }
                    AliasValue::KeywordAliases(v) => {
                        for (f, n) in v {
                            cx.form("ALLA pair word 0", f);
                            cx.alias("ALLA pair word 1", Some(key), i64::from(*n));
                        }
                    }
                    AliasValue::Condition(_) => conditions += 1,
                }
            }
            let mut k: Vec<&str> = kinds.into_iter().collect();
            if conditions > 0 {
                k.push("conditions");
            }
            let k = if k.is_empty() {
                "none of them".to_string()
            } else {
                k.join("+")
            };
            bump(
                &mut fill,
                format!("{kind} aliases by fill subrecords present\t{k}"),
            );
        }
        // Stage fragments of the VMAD block against the record's stages.
        let Some(v) = vmad else {
            let what = if q.vmad.is_some() {
                "quests whose VMAD is not read to its end"
            } else {
                "quests without a VMAD"
            };
            bump(
                &mut frag,
                format!("{what}, {} stages", bucket(q.stages.len())),
            );
            continue;
        };
        let Some(block) = &v.quest else {
            bump(&mut frag, "quests with a VMAD and no quest block");
            continue;
        };
        bump(&mut frag, "quests with a quest block");
        if block.script.name.is_empty() {
            bump(&mut frag, "quest blocks with an empty fragment script name");
        } else if v.scripts.iter().any(|s| s.name == block.script.name) {
            bump(
                &mut frag,
                "quest blocks whose fragment script is also in the VMAD script list",
            );
        } else {
            bump(
                &mut frag,
                "quest blocks whose fragment script is not in the VMAD script list",
            );
        }
        let mut pairs = BTreeSet::new();
        let mut per_stage: BTreeMap<u16, u64> = BTreeMap::new();
        for f in &block.fragments {
            bump(&mut frag, "fragments");
            if f.script != block.script.name {
                bump(
                    &mut frag,
                    "fragments naming another script than the block's",
                );
            }
            bump(
                &mut frag,
                format!("fragment u16 after the stage = {}", f.word),
            );
            bump(
                &mut frag,
                format!("fragment byte before the texts = {}", f.byte),
            );
            *per_stage.entry(f.stage).or_default() += 1;
            let what = match q.stage(f.stage) {
                None => "stage not in the record".to_string(),
                Some(s) => {
                    let n = s.entries.len();
                    match usize::try_from(f.log_entry) {
                        Ok(i) if i < n => {
                            let e = &s.entries[i];
                            let earlier = s.entries[..i].iter().any(|x| !x.conditions.is_empty());
                            bump(
                                &mut frag,
                                format!(
                                    "mapped fragments: entry has conditions {}, an earlier entry has conditions {}, stage has {} entries",
                                    !e.conditions.is_empty(),
                                    earlier,
                                    bucket(n)
                                ),
                            );
                            "stage and log entry in the record".into()
                        }
                        Ok(_) => format!(
                            "stage in the record, log entry index past its {} entries",
                            bucket(n)
                        ),
                        Err(_) => "stage in the record, negative log entry index".into(),
                    }
                }
            };
            bump(&mut frag, format!("fragment maps: {what}"));
            if !pairs.insert((f.stage, f.log_entry)) {
                bump(
                    &mut frag,
                    "fragments repeating a (stage, log entry) pair of the quest",
                );
            }
        }
        for n in per_stage.values() {
            bump(
                &mut frag,
                format!("stages with fragments: {} fragments", bucket(*n as usize)),
            );
        }
        for s in &q.stages {
            if !per_stage.contains_key(&s.stage) {
                bump(
                    &mut frag,
                    "stages of quests with a block, without a fragment",
                );
            }
        }
        let alias_props = block
            .script
            .properties
            .iter()
            .filter(
                |p| matches!(p.value, fo4_plugin::vmad::PropertyValue::Object(o) if o.alias >= 0),
            )
            .count();
        add(
            &mut frag,
            "fragment script properties holding an alias value",
            alias_props as u64,
        );
        for p in &block.script.properties {
            if let fo4_plugin::vmad::PropertyValue::Object(o) = p.value {
                if o.alias >= 0 {
                    let same = o.form_id == 0 || {
                        let plugin = winners
                            .iter()
                            .find(|w| &w.0 == key)
                            .map(|w| w.1)
                            .unwrap_or(0);
                        store
                            .order()
                            .remap(plugin, o.form_id, "", 0)
                            .ok()
                            .flatten()
                            .as_ref()
                            == Some(key)
                    };
                    let forced = q
                        .alias(o.alias as u32)
                        .and_then(|a| a.forced_reference())
                        .is_some();
                    bump(
                        &mut frag,
                        format!(
                            "fragment alias properties: of the same quest {same}, its alias has ALFR {}",
                            same && forced
                        ),
                    );
                }
            }
        }
    }
    let mut out = String::new();
    writeln!(
        out,
        "# quest_census: every winning QUST read with fo4_plugin::quest (spec/quest.md)"
    )?;
    writeln!(
        out,
        "# counts only: tags, record types and numbers; no name, FormID or text"
    )?;
    writeln!(out, "{}\twinning QUST records", winners.len())?;
    writeln!(out, "{}\tread to the last byte", quests.len())?;
    for (k, v) in &errors {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(
        out,
        "\n## bytes and subrecords by tag (typed: given a field; raw: kept as stored)"
    )?;
    for (k, v) in &bytes {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(
        out,
        "\n## subrecord order observed: tag followed by tag (^ start, $ end)"
    )?;
    for (k, v) in &trans {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(out, "\n## values, flags, alias index and FormID checks")?;
    for (k, v) in &cx.c {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(out, "\n## alias fill subrecords")?;
    for (k, v) in &fill {
        writeln!(out, "{v}\t{k}")?;
    }
    writeln!(
        out,
        "\n## stage fragments of the VMAD quest block against the record"
    )?;
    for (k, v) in &frag {
        writeln!(out, "{v}\t{k}")?;
    }
    std::fs::write(&a[1], out)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
